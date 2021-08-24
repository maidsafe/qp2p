// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::connection_pool::ConnId;

use super::error::Error;
#[cfg(not(feature = "no-igd"))]
use super::igd::forward_port;
use super::wire_msg::WireMsg;
use super::{
    config::{Config, InternalConfig},
    connection_deduplicator::{ConnectionDeduplicator, DedupHandle},
    connection_pool::ConnectionPool,
    connections::{
        listen_for_incoming_connections, listen_for_incoming_messages, Connection,
        DisconnectionEvents, RecvStream, SendStream,
    },
    error::{ClientEndpointError, ConnectionError, Result, SendError},
};
use bytes::Bytes;
use std::net::SocketAddr;

use backoff::{future::retry, ExponentialBackoff};
use tokio::sync::broadcast::{self, Receiver, Sender};
use tokio::sync::mpsc::{self, Receiver as MpscReceiver, Sender as MpscSender};
use tokio::time::{timeout, Duration};
use tracing::{debug, error, info, trace, warn};

/// Host name of the Quic communication certificate used by peers
// FIXME: make it configurable
const CERT_SERVER_NAME: &str = "MaidSAFE.net";

// Number of seconds before timing out the IGD request to forward a port.
#[cfg(not(feature = "no-igd"))]
const PORT_FORWARD_TIMEOUT: u64 = 30;

// Number of seconds before timing out the echo service query.
const ECHO_SERVICE_QUERY_TIMEOUT: u64 = 30;

/// Standard size of our channel bounds
const STANDARD_CHANNEL_SIZE: usize = 10000;

/// Channel on which incoming messages can be listened to
pub struct IncomingMessages(pub(crate) MpscReceiver<(SocketAddr, Bytes)>);

impl IncomingMessages {
    /// Blocks and returns the next incoming message and the source peer address
    pub async fn next(&mut self) -> Option<(SocketAddr, Bytes)> {
        self.0.recv().await
    }
}

/// Channel on which incoming connections are notified on
pub struct IncomingConnections(pub(crate) MpscReceiver<SocketAddr>);

impl IncomingConnections {
    /// Blocks until there is an incoming connection and returns the address of the
    /// connecting peer
    pub async fn next(&mut self) -> Option<SocketAddr> {
        self.0.recv().await
    }
}

/// Endpoint instance which can be used to create connections to peers,
/// and listen to incoming messages from other peers.
#[derive(Clone)]
pub struct Endpoint<I: ConnId> {
    local_addr: SocketAddr,
    public_addr: Option<SocketAddr>,
    quic_endpoint: quinn::Endpoint,
    message_tx: MpscSender<(SocketAddr, Bytes)>,
    disconnection_tx: MpscSender<SocketAddr>,
    bootstrap_nodes: Vec<SocketAddr>,
    config: InternalConfig,
    termination_tx: Sender<()>,
    connection_pool: ConnectionPool<I>,
    connection_deduplicator: ConnectionDeduplicator,
}

impl<I: ConnId> std::fmt::Debug for Endpoint<I> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Endpoint")
            .field("local_addr", &self.local_addr)
            .field("quic_endpoint", &"<endpoint omitted>")
            .field("config", &self.config)
            .finish()
    }
}

impl<I: ConnId> Endpoint<I> {
    /// Create a client endpoint at the given address.
    ///
    /// A client endpoint cannot receive incoming connections, as such they also do not need to be
    /// publicly reachable. They can still communicate over outgoing connections and receive
    /// incoming streams, since QUIC allows for either side of a connection to initiate streams.
    pub fn new_client(
        local_addr: impl Into<SocketAddr>,
        config: Config,
    ) -> Result<Self, ClientEndpointError> {
        let config = InternalConfig::try_from_config(config)?;

        let local_addr = local_addr.into();
        let (quic_endpoint, _) = quinn::Endpoint::builder().bind(&local_addr)?;
        let local_addr = quic_endpoint
            .local_addr()
            .map_err(ClientEndpointError::Socket)?;

        let (message_tx, _) = mpsc::channel(STANDARD_CHANNEL_SIZE);
        let (disconnection_tx, _) = mpsc::channel(STANDARD_CHANNEL_SIZE);
        let (termination_tx, _) = broadcast::channel(1);

        let endpoint = Self {
            local_addr,
            public_addr: None,
            quic_endpoint,
            message_tx,
            disconnection_tx,
            bootstrap_nodes: Default::default(),
            config,
            termination_tx,
            connection_pool: ConnectionPool::new(),
            connection_deduplicator: ConnectionDeduplicator::new(),
        };

        Ok(endpoint)
    }

    pub(crate) async fn new(
        quic_endpoint: quinn::Endpoint,
        quic_incoming: quinn::Incoming,
        bootstrap_nodes: Vec<SocketAddr>,
        config: InternalConfig,
    ) -> Result<(
        Self,
        IncomingConnections,
        IncomingMessages,
        DisconnectionEvents,
    )> {
        let local_addr = quic_endpoint.local_addr()?;
        let public_addr = match (config.external_ip, config.external_port) {
            (Some(ip), Some(port)) => Some(SocketAddr::new(ip, port)),
            _ => None,
        };
        let connection_pool = ConnectionPool::new();

        let (message_tx, message_rx) = mpsc::channel(STANDARD_CHANNEL_SIZE);
        let (connection_tx, connection_rx) = mpsc::channel(STANDARD_CHANNEL_SIZE);
        let (disconnection_tx, disconnection_rx) = mpsc::channel(STANDARD_CHANNEL_SIZE);
        let (termination_tx, termination_rx) = broadcast::channel(1);

        let mut endpoint = Self {
            local_addr,
            public_addr,
            quic_endpoint,
            message_tx: message_tx.clone(),
            disconnection_tx: disconnection_tx.clone(),
            bootstrap_nodes,
            config,
            termination_tx,
            connection_pool: connection_pool.clone(),
            connection_deduplicator: ConnectionDeduplicator::new(),
        };

        if let Some(addr) = endpoint.public_addr {
            // External IP and port number is provided
            // This means that the user has performed manual port-forwarding
            // Verify that the given socket address is reachable
            if let Some(contact) = endpoint.bootstrap_nodes.get(0) {
                info!("Verifying provided public IP address");
                let connection = endpoint.get_or_connect_to(contact).await?;
                let (mut send, mut recv) = connection.open_bi(0).await?;
                send.send(WireMsg::EndpointVerificationReq(addr)).await?;
                let response = timeout(
                    Duration::from_secs(ECHO_SERVICE_QUERY_TIMEOUT),
                    WireMsg::read_from_stream(&mut recv.quinn_recv_stream),
                )
                .await;
                match response {
                    Ok(Ok(WireMsg::EndpointVerificationResp(valid))) => {
                        if valid {
                            info!("Endpoint verification successful! {} is reachable.", addr);
                        } else {
                            error!("Endpoint verification failed! {} is not reachable.", addr);
                            return Err(Error::IncorrectPublicAddress);
                        }
                    }
                    Ok(Ok(other)) => {
                        error!(
                            "Unexpected message when verifying public endpoint: {}",
                            other
                        );
                        return Err(Error::UnexpectedMessageType(other.into()));
                    }
                    Ok(Err(err)) => {
                        error!("Error while verifying Public IP Address");
                        return Err(err);
                    }
                    Err(err) => {
                        error!(
                            "Timeout while trying to validate Public IP address: {}",
                            err
                        );
                        return Err(Error::IncorrectPublicAddress);
                    }
                }
            } else {
                warn!("Public IP address not verified since bootstrap contacts are empty");
            }
        } else {
            endpoint.public_addr = Some(endpoint.fetch_public_address(termination_rx).await?);
        }

        listen_for_incoming_connections(
            quic_incoming,
            connection_pool,
            message_tx,
            connection_tx,
            disconnection_tx,
            endpoint.clone(),
        );

        Ok((
            endpoint,
            IncomingConnections(connection_rx),
            IncomingMessages(message_rx),
            DisconnectionEvents(disconnection_rx),
        ))
    }

    /// Endpoint local address
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Get the public address of the endpoint.
    pub fn public_addr(&self) -> SocketAddr {
        self.public_addr.unwrap_or(self.local_addr)
    }

    /// Get our connection address to give to others for them to connect to us.
    ///
    /// Attempts to use UPnP to automatically find the public endpoint and forward a port.
    /// Will use hard coded contacts to ask for our endpoint. If no contact is given then we'll
    /// simply build our connection info by querying the underlying bound socket for our address.
    /// Note that if such an obtained address is of unspecified category we will ignore that as
    /// such an address cannot be reached and hence not useful.
    async fn fetch_public_address(
        &mut self,
        #[allow(unused)] termination_rx: Receiver<()>,
    ) -> Result<SocketAddr> {
        // Skip port forwarding
        if self.local_addr.ip().is_loopback() || !self.config.forward_port {
            self.public_addr = Some(self.local_addr);
        }

        if let Some(socket_addr) = self.public_addr {
            return Ok(socket_addr);
        }

        let mut addr = None;

        #[cfg(feature = "no-igd")]
        if self.config.forward_port {
            warn!("Ignoring 'forward_port' flag from config since IGD has been disabled (feature 'no-igd' has been set)");
        }

        // Try to contact an echo service
        match timeout(
            Duration::from_secs(ECHO_SERVICE_QUERY_TIMEOUT),
            self.query_ip_echo_service(),
        )
        .await
        {
            Ok(Ok(echo_res)) => addr = Some(echo_res),
            Ok(Err(err)) => info!("Could not contact echo service: {} - {:?}", err, err),
            Err(err) => info!("Query to echo service timed out: {:?}", err),
        }

        #[cfg(not(feature = "no-igd"))]
        if self.config.forward_port {
            // Attempt to use IGD for port forwarding
            match timeout(
                Duration::from_secs(PORT_FORWARD_TIMEOUT),
                forward_port(
                    self.local_addr,
                    self.config.upnp_lease_duration,
                    termination_rx,
                ),
            )
            .await
            {
                Ok(res) => match res {
                    Ok(port) => {
                        if let Some(socket) = &mut addr {
                            socket.set_port(port);
                        }
                    }
                    Err(e) => {
                        info!("IGD request failed: {} - {:?}", e, e);
                    }
                },
                Err(e) => {
                    info!("IGD request timeout: {:?}", e);
                }
            }
        }

        addr.map_or(Err(Error::UnresolvedPublicIp), |socket_addr| {
            self.public_addr = Some(socket_addr);
            Ok(socket_addr)
        })
    }

    /// Removes all existing connections to a given peer
    pub async fn disconnect_from(&self, peer_addr: &SocketAddr) {
        self.connection_pool
            .remove(peer_addr)
            .await
            .iter()
            .for_each(|conn| {
                conn.close(0u8.into(), b"");
            });
    }

    /// Connects to another peer, retries for `config.retry_duration_msec` if the connection fails.
    ///
    /// **Note:** this method is intended for use when it's necessary to connect to a specific peer.
    /// See [`connect_to_any`](Self::connect_to_any) if you just need a connection with any of a set
    /// of peers.
    ///
    /// Returns `Connection` which is a handle for sending messages to the peer and
    /// `IncomingMessages` which is a stream of messages received from the peer.
    /// The incoming messages stream might be `None`. See the next section for more info.
    ///
    /// # Connection pooling
    ///
    /// Connection are stored in an internal pool and reused if possible. A connection remains in
    /// the pool while its `IncomingMessages` instances exists and while the connection is open.
    ///
    /// When a new connection is established, this function returns both the `Connection` instance
    /// and the `IncomingMessages` stream. If an existing connection is retrieved from the pool,
    /// the incoming messages will be `None`. Multiple `Connection` instances can exists
    /// simultaneously and they all share the same underlying connection resource. On the other
    /// hand, at most one `IncomingMessages` stream can exist per peer.
    ///
    /// How to handle the `IncomingMessages` depends on the networking model of the application:
    ///
    /// In the peer-to-peer model, where peers can arbitrarily send and receive messages to/from
    /// other peers, it is recommended to keep the `IncomingMessages` around and listen on it for
    /// new messages by repeatedly calling `next` and only drop it when it returns `None`.
    /// On the other hand, there is no need to keep `Connection` around as it can be cheaply
    /// retrieved again when needed by calling `connect_to`. When the connection gets closed by the
    /// peer or it timeouts due to inactivity, the incoming messages stream gets closed and once
    /// it's dropped the connection gets removed from the pool automatically. Calling `connect_to`
    /// afterwards will open a new connection.
    ///
    /// In the client-server model, where only the client send requests to the server and then
    /// listens for responses and never the other way around, it's OK to ignore (drop) the incoming
    /// messages stream and only use bi-directional streams obtained by calling
    /// `Connection::open_bi`. In this case the connection won't be pooled and the application is
    /// responsible for caching it.
    ///
    /// When sending a message on `Connection` fails, the connection is also automatically removed
    /// from the pool and the subsequent call to `connect_to` is guaranteed to reopen new connection
    /// too.
    pub async fn connect_to(&self, node_addr: &SocketAddr) -> Result<(), ConnectionError> {
        let _ = self.get_or_connect_to(node_addr).await?;
        Ok(())
    }

    /// Connect to any of the given peers.
    ///
    /// Often in peer-to-peer networks, it's sufficient to communicate to any node on the network,
    /// rather than having to connect to specific nodes. This method will start connecting to every
    /// peer in `peer_addrs`, and return the address of the first successfully established
    /// connection (the rest are cancelled and discarded). All connection attempts will be retried
    /// for `config.retry_duration_msec` on failure.
    ///
    /// The successful connection, if any, will be stored in the connection pool (see
    /// [`connect_to`](Self::connect_to) for more info on connection pooling).
    pub async fn connect_to_any(&self, peer_addrs: &[SocketAddr]) -> Option<SocketAddr> {
        trace!("Connecting to any of {:?}", peer_addrs);
        if peer_addrs.is_empty() {
            return None;
        }

        // Attempt to create a new connection to all nodes and return the first one to succeed
        let tasks = peer_addrs
            .iter()
            .map(|addr| Box::pin(self.get_or_connect_to(addr)));

        match futures::future::select_ok(tasks).await {
            Ok((connection, _)) => Some(connection.remote_address()),
            Err(error) => {
                error!("Failed to bootstrap to the network, last error: {}", error);
                None
            }
        }
    }

    /// Verify if an address is publicly reachable. This will attempt to create
    /// a new connection and use it to exchange a message and verify that the node
    /// can be reached.
    pub async fn is_reachable(&self, peer_addr: &SocketAddr) -> Result<()> {
        trace!("Checking is reachable");
        let connection = self.get_or_connect_to(peer_addr).await?;
        let (mut send_stream, mut recv_stream) = connection.open_bi(0).await?;

        send_stream.send(WireMsg::EndpointEchoReq).await?;

        match timeout(
            Duration::from_secs(ECHO_SERVICE_QUERY_TIMEOUT),
            recv_stream.next_wire_msg(),
        )
        .await
        {
            Ok(Ok(WireMsg::EndpointEchoResp(_))) => Ok(()),
            Ok(Ok(other)) => {
                info!(
                    "Unexpected message type when verifying reachability: {}",
                    &other
                );
                Ok(())
            }
            Ok(Err(err)) => {
                info!("Unable to contact peer: {:?}", err);
                Err(err)
            }
            Err(err) => {
                info!("Unable to contact peer: {:?}", err);
                Err(Error::NoEchoServiceResponse)
            }
        }
    }

    /// Get the connection ID of an existing connection with the provided socket address
    pub async fn get_connection_id(&self, addr: &SocketAddr) -> Option<I> {
        self.connection_pool
            .get_by_addr(addr)
            .await
            .map(|(_, remover)| remover.id())
    }

    /// Get the SocketAddr of a connection using the connection ID
    pub async fn get_socket_addr_by_id(&self, addr: &I) -> Option<SocketAddr> {
        self.connection_pool
            .get_by_id(addr)
            .await
            .map(|(_, remover)| *remover.remote_addr())
    }

    /// Open a bi-directional peer with a given peer
    /// Priority default is 0. Both lower and higher can be passed in.
    pub async fn open_bidirectional_stream(
        &self,
        peer_addr: &SocketAddr,
        priority: i32,
    ) -> Result<(SendStream, RecvStream), ConnectionError> {
        let connection = self.get_or_connect_to(peer_addr).await?;
        connection.open_bi(priority).await
    }

    /// Sends a message to a peer. This will attempt to use an existing connection
    /// to the destination  peer. If a connection does not exist, this will fail with `Error::MissingConnection`
    /// Priority default is 0. Both lower and higher can be passed in.
    pub async fn try_send_message(
        &self,
        msg: Bytes,
        dest: &SocketAddr,
        priority: i32,
    ) -> Result<()> {
        if let Some((conn, guard)) = self.connection_pool.get_by_addr(dest).await {
            trace!("Connection exists in the connection pool: {}", dest);
            let connection = Connection::new(conn, guard);
            connection.send_uni(msg, priority).await?;
            Ok(())
        } else {
            Err(Error::MissingConnection)
        }
    }

    /// Sends a message to a peer. This will attempt to use an existing connection
    /// to the peer first. If this connection is broken or doesn't exist
    /// a new connection is created and the message is sent.
    /// Priority default is 0. Both lower and higher can be passed in.
    pub async fn send_message(
        &self,
        msg: Bytes,
        dest: &SocketAddr,
        priority: i32,
    ) -> Result<(), SendError> {
        let connection = self.get_or_connect_to(dest).await?;
        self.retry(|| async { Ok(connection.send_uni(msg.clone(), priority).await?) })
            .await?;

        Ok(())
    }

    /// Close all the connections of this endpoint immediately and stop accepting new connections.
    pub fn close(&self) {
        let _ = self.termination_tx.send(());
        self.quic_endpoint.close(0_u32.into(), b"")
    }

    // Private helper
    async fn query_ip_echo_service(&self) -> Result<SocketAddr> {
        // Bail out early if we don't have any contacts.
        if self.bootstrap_nodes.is_empty() {
            return Err(Error::NoEchoServerEndpointDefined);
        }

        let mut tasks = Vec::default();
        for node in self.bootstrap_nodes.iter().cloned() {
            let endpoint = self.clone();
            let task_handle = tokio::spawn(async move {
                debug!("Connecting to {:?}", &node);
                let connection = endpoint.get_or_connect_to(&node).await?;
                let (mut send_stream, mut recv_stream) = connection.open_bi(0).await?;
                send_stream.send(WireMsg::EndpointEchoReq).await?;
                match WireMsg::read_from_stream(&mut recv_stream.quinn_recv_stream).await {
                    Ok(WireMsg::EndpointEchoResp(socket_addr)) => Ok(socket_addr),
                    Ok(msg) => Err(Error::UnexpectedMessageType(msg.into())),
                    Err(err) => Err(err),
                }
            });
            tasks.push(task_handle);
        }

        let (result, _) = futures::future::select_ok(tasks).await.map_err(|err| {
            error!("Failed to contact echo service: {}", err);
            Error::EchoServiceFailure(err.to_string())
        })?;

        result
    }

    /// Returns the list of boostrap nodes that the endpoint will try bootstrapping to.
    /// This is the combined list of contacts from the Hard Coded contacts in the config
    /// and the bootstrap cache (if enabled)
    pub fn bootstrap_nodes(&self) -> &[SocketAddr] {
        &self.bootstrap_nodes
    }

    /// Get a connection from the pool, or create one, for the given `addr`.
    pub(crate) async fn get_or_connect_to(
        &self,
        addr: &SocketAddr,
    ) -> Result<Connection<I>, ConnectionError> {
        let completion = loop {
            if let Some((conn, remover)) = self.connection_pool.get_by_addr(addr).await {
                trace!("We are already connected to this peer: {}", addr);
                return Ok(Connection::new(conn, remover));
            }

            // Check if a connect attempt to this address is already in progress.
            match self.connection_deduplicator.query(addr).await {
                DedupHandle::Dup(Ok(())) => continue, // the connection should now be in the pool
                DedupHandle::Dup(Err(error)) => return Err(error), // cannot connect
                DedupHandle::New(completion) => break completion,
            }
        };

        match self.new_connection(addr).await {
            Ok(new_connection) => {
                trace!("Successfully connected to peer: {}", addr);

                let connection = new_connection.connection;
                let id = ConnId::generate(&connection.remote_address());
                let remover = self
                    .connection_pool
                    .insert(id, connection.remote_address(), connection.clone())
                    .await;

                listen_for_incoming_messages(
                    new_connection.uni_streams,
                    new_connection.bi_streams,
                    remover.clone(),
                    self.message_tx.clone(),
                    self.disconnection_tx.clone(),
                    self.clone(),
                );

                let _ = completion.complete(Ok(()));
                Ok(Connection::new(connection, remover))
            }
            Err(error) => {
                let _ = completion.complete(Err(error.clone()));
                Err(error)
            }
        }
    }

    /// Attempt a connection to a node_addr.
    ///
    /// All failures are retried with exponential back-off. This doesn't use the connection pool, it
    /// will always try to open a new connection.
    async fn new_connection(
        &self,
        node_addr: &SocketAddr,
    ) -> Result<quinn::NewConnection, ConnectionError> {
        self.retry(|| async {
            trace!("Attempting to connect to {:?}", node_addr);
            let connecting = match self.quic_endpoint.connect_with(
                self.config.client.clone(),
                node_addr,
                CERT_SERVER_NAME,
            ) {
                Ok(conn) => Ok(conn),
                Err(error) => {
                    warn!("Connection attempt failed due to {:?}", error);
                    Err(ConnectionError::from(error))
                }
            }?;

            let new_conn = match connecting.await {
                Ok(new_conn) => {
                    debug!("okay was had");
                    Ok(new_conn)
                }
                Err(error) => {
                    error!("some error: {:?}", error);
                    Err(ConnectionError::from(error))
                }
            }?;

            Ok(new_conn)
        })
        .await
    }

    fn retry<R, E, Fn, Fut>(&self, op: Fn) -> impl futures::Future<Output = Result<R, E>>
    where
        Fn: FnMut() -> Fut,
        Fut: futures::Future<Output = Result<R, backoff::Error<E>>>,
    {
        let backoff = ExponentialBackoff {
            max_elapsed_time: Some(self.config.min_retry_duration),
            ..Default::default()
        };
        retry(backoff, op)
    }
}
