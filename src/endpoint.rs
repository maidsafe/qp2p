// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use super::error::Error;
use super::igd::forward_port;
use super::wire_msg::WireMsg;
use super::{
    api::DEFAULT_UPNP_LEASE_DURATION_SEC,
    connection_deduplicator::ConnectionDeduplicator,
    connection_pool::ConnectionPool,
    connections::{listen_for_incoming_connections, listen_for_incoming_messages, Connection},
    error::Result,
    Config,
};
use bytes::Bytes;
use futures::lock::Mutex;
use log::{debug, error, info, trace, warn};
use std::collections::VecDeque;
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::sync::mpsc;
use tokio::sync::mpsc::UnboundedSender;
use tokio::time::timeout;

/// Host name of the Quic communication certificate used by peers
// FIXME: make it configurable
const CERT_SERVER_NAME: &str = "MaidSAFE.net";

/// Endpoint instance which can be used to create connections to peers,
/// and listen to incoming messages from other peers.
pub struct Endpoint {
    local_addr: SocketAddr,
    public_addr: Option<SocketAddr>,
    quic_endpoint: quinn::Endpoint,
    message_queue: Arc<Mutex<VecDeque<(SocketAddr, Bytes)>>>,
    message_tx: UnboundedSender<(SocketAddr, Bytes)>,
    connection_queue: Arc<Mutex<VecDeque<SocketAddr>>>,
    client_cfg: quinn::ClientConfig,
    bootstrap_nodes: Vec<SocketAddr>,
    qp2p_config: Config,
    connection_pool: ConnectionPool,
    connection_deduplicator: ConnectionDeduplicator,
}

impl std::fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Endpoint")
            .field("local_addr", &self.local_addr)
            .field("quic_endpoint", &"<endpoint omitted>")
            .field("client_cfg", &self.client_cfg)
            .finish()
    }
}

impl Endpoint {
    pub(crate) async fn new(
        quic_endpoint: quinn::Endpoint,
        quic_incoming: quinn::Incoming,
        client_cfg: quinn::ClientConfig,
        bootstrap_nodes: Vec<SocketAddr>,
        qp2p_config: Config,
    ) -> Result<Self> {
        let local_addr = quic_endpoint.local_addr()?;
        let public_addr = match (qp2p_config.external_ip, qp2p_config.external_port) {
            (Some(ip), Some(port)) => Some(SocketAddr::new(ip, port)),
            _ => None,
        };
        let connection_pool = ConnectionPool::new();
        // Should we bound these
        let (message_tx, mut message_rx) = mpsc::unbounded_channel();
        let messages = Arc::new(Mutex::new(VecDeque::new()));
        let (connection_tx, mut connection_rx) = mpsc::unbounded_channel();
        let connections = Arc::new(Mutex::new(VecDeque::new()));
        let endpoint = Self {
            local_addr,
            public_addr,
            quic_endpoint,
            message_queue: messages.clone(),
            message_tx: message_tx.clone(),
            connection_queue: connections.clone(),
            client_cfg,
            bootstrap_nodes,
            qp2p_config,
            connection_pool: connection_pool.clone(),
            connection_deduplicator: ConnectionDeduplicator::new(),
        };
        if let Some(addr) = endpoint.public_addr {
            // External IP and port number is provided
            // This means that the user has performed manual port-forwarding
            // Verify that the given socket address is reachable
            if let Some(contact) = endpoint.bootstrap_nodes.iter().next() {
                info!("Verifying provided public IP address");
                endpoint.connect_to(contact).await?;
                let connection = endpoint
                    .get_connection(&contact)
                    .ok_or_else(|| Error::BootstrapFailure)?; //FIXME
                let (mut send, mut recv) = connection.open_bi().await?;
                send.send(WireMsg::EndpointVerificationReq(addr)).await?;
                let response = WireMsg::read_from_stream(&mut recv.quinn_recv_stream).await?;
                match response {
                    WireMsg::EndpointVerficationResp(valid) => {
                        if valid {
                            info!("Endpoint verification successful! {} is reachable.", addr);
                        // Ok(endpoint)
                        } else {
                            error!("Endpoint verification failed! {} is not reachable.", addr);
                            return Err(Error::IncorrectPublicAddress);
                        }
                    }
                    other => {
                        error!(
                            "Unexpected message when verifying public endpoint: {}",
                            other
                        );
                        return Err(Error::UnexpectedMessageType(other));
                    }
                }
            } else {
                warn!("Public IP address not verified since bootstrap contacts are empty");
            }
        }
        listen_for_incoming_connections(quic_incoming, connection_pool, message_tx, connection_tx);
        let _ = tokio::spawn(async move {
            while let Some((source, message)) = message_rx.recv().await {
                let mut message_queue = messages.lock().await;
                message_queue.push_back((source, message));
            }
        });
        let _ = tokio::spawn(async move {
            while let Some(new_peer) = connection_rx.recv().await {
                let mut connection_queue = connections.lock().await;
                connection_queue.push_back(new_peer);
            }
        });
        Ok(endpoint)
    }

    /// Endpoint local address
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Returns the socket address of the endpoint
    pub async fn socket_addr(&mut self) -> Result<SocketAddr> {
        if cfg!(test) || !self.qp2p_config.forward_port {
            Ok(self.local_addr())
        } else {
            self.public_addr().await
        }
    }

    /// Get our connection adddress to give to others for them to connect to us.
    ///
    /// Attempts to use UPnP to automatically find the public endpoint and forward a port.
    /// Will use hard coded contacts to ask for our endpoint. If no contact is given then we'll
    /// simply build our connection info by querying the underlying bound socket for our address.
    /// Note that if such an obtained address is of unspecified category we will ignore that as
    /// such an address cannot be reached and hence not useful.
    pub async fn public_addr(&mut self) -> Result<SocketAddr> {
        // Skip port forwarding
        if self.local_addr.ip().is_loopback() {
            return Ok(self.local_addr);
        }

        if let Some(socket_addr) = self.public_addr {
            return Ok(socket_addr);
        }

        let mut addr = None;

        if self.qp2p_config.forward_port {
            // Attempt to use IGD for port forwarding
            match timeout(
                Duration::from_secs(30),
                forward_port(
                    self.local_addr,
                    self.qp2p_config
                        .upnp_lease_duration
                        .unwrap_or(DEFAULT_UPNP_LEASE_DURATION_SEC),
                ),
            )
            .await
            {
                Ok(res) => match res {
                    Ok(public_sa) => {
                        debug!("IGD success: {:?}", SocketAddr::V4(public_sa));
                        addr = Some(SocketAddr::V4(public_sa));
                    }
                    Err(e) => {
                        info!("IGD request failed: {} - {:?}", e, e);
                        return Err(Error::IgdNotSupported);
                    }
                },
                Err(e) => {
                    info!("IGD request timeout: {:?}", e);
                    return Err(Error::IgdNotSupported);
                }
            }
        }

        // Try to contact an echo service
        match timeout(Duration::from_secs(30), self.query_ip_echo_service()).await {
            Ok(res) => match res {
                Ok(echo_res) => match addr {
                    None => {
                        addr = Some(echo_res);
                    }
                    Some(address) => {
                        info!("Got response from echo service: {:?}, but IGD has already provided our external address: {:?}", echo_res, address);
                    }
                },
                Err(err) => {
                    info!("Could not contact echo service: {} - {:?}", err, err);
                }
            },
            Err(e) => info!("Echo service timed out: {:?}", e),
        }

        addr.map_or(Err(Error::NoEchoServiceResponse), |socket_addr| {
            self.public_addr = Some(socket_addr);
            Ok(socket_addr)
        })
    }

    /// Connects to another peer.
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
    pub async fn connect_to(&self, node_addr: &SocketAddr) -> Result<()> {
        if self.connection_pool.has(node_addr) {
            trace!("We are already connected to this peer: {}", node_addr);
        }

        // Check if a connect attempt to this address is already in progress.
        match self.connection_deduplicator.query(node_addr).await {
            Some(Ok(())) => return Ok(()),
            Some(Err(error)) => return Err(error.into()),
            None => {}
        }

        // This is the first attempt - proceed with establishing the connection now.
        let connecting = match self.quic_endpoint.connect_with(
            self.client_cfg.clone(),
            node_addr,
            CERT_SERVER_NAME,
        ) {
            Ok(connecting) => connecting,
            Err(error) => {
                self.connection_deduplicator
                    .complete(node_addr, Err(error.clone().into()))
                    .await;
                return Err(error.into());
            }
        };

        let new_conn = match connecting.await {
            Ok(new_conn) => new_conn,
            Err(error) => {
                self.connection_deduplicator
                    .complete(node_addr, Err(error.clone().into()))
                    .await;
                return Err(error.into());
            }
        };

        trace!("Successfully connected to peer: {}", node_addr);

        let guard = self
            .connection_pool
            .insert(*node_addr, new_conn.connection.clone());

        listen_for_incoming_messages(
            new_conn.uni_streams,
            new_conn.bi_streams,
            guard,
            self.message_tx.clone(),
        );

        self.connection_deduplicator
            .complete(node_addr, Ok(()))
            .await;

        Ok(())
    }

    /// Get an existing connection for the peer address.
    fn get_connection(&self, peer_addr: &SocketAddr) -> Option<Connection> {
        if let Some((conn, guard)) = self.connection_pool.get(peer_addr) {
            trace!("Using cached connection to peer: {}", peer_addr);
            Some(Connection::new(conn, guard))
        } else {
            None
        }
    }

    /// Sends a message to a peer. This will attempt to re-use any existing connections
    /// with the said peer. If a connection doesn't exist already, a new connection will be created.
    pub async fn send_message(&self, msg: Bytes, dest: &SocketAddr) -> Result<()> {
        self.connect_to(dest).await?;

        let connection = self
            .get_connection(dest)
            .ok_or_else(|| Error::MissingConnection)?; // will never be None
        connection.send_uni(msg).await?;
        Ok(())
    }

    /// Close all the connections of this endpoint immediately and stop accepting new connections.
    pub fn close(&self) {
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
            debug!("Connecting to {:?}", &node);
            self.connect_to(&node).await?;
            let connection = self
                .get_connection(&node)
                .ok_or_else(|| Error::MissingConnection)?;
            let task_handle = tokio::spawn(async move {
                let (mut send_stream, mut recv_stream) = connection.open_bi().await?;
                send_stream.send(WireMsg::EndpointEchoReq).await?;
                match WireMsg::read_from_stream(&mut recv_stream.quinn_recv_stream).await {
                    Ok(WireMsg::EndpointEchoResp(socket_addr)) => Ok(socket_addr),
                    Ok(msg) => Err(Error::UnexpectedMessageType(msg)),
                    Err(err) => Err(err),
                }
            });
            tasks.push(task_handle);
        }

        let (result, _) = futures::future::select_ok(tasks).await.map_err(|err| {
            log::error!("Failed to contact echo service: {}", err);
            Error::EchoServiceFailure(err.to_string())
        })?;
        result
    }

    pub(crate) fn bootstrap_nodes(&self) -> &[SocketAddr] {
        &self.bootstrap_nodes
    }

    /// Returns the socket address of a peer that has connected to us
    pub async fn next_incoming_connection(&mut self) -> Option<SocketAddr> {
        self.connection_queue.lock().await.pop_front()
    }

    /// Returns a message received from another peer along with the source address
    pub async fn next_incoming_message(&mut self) -> Option<(SocketAddr, Bytes)> {
        self.message_queue.lock().await.pop_front()
    }
}
