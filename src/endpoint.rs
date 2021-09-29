// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

#[cfg(feature = "igd")]
use super::igd::{forward_port, IgdError};
use super::wire_msg::WireMsg;
use super::{
    config::{Config, InternalConfig, SERVER_NAME},
    connection_deduplicator::{ConnectionDeduplicator, DedupHandle},
    connection_pool::{ConnId, ConnectionPool, ConnectionRemover},
    connections::{
        listen_for_incoming_connections, listen_for_incoming_messages, Connection,
        DisconnectionEvents, RecvStream, SendStream,
    },
    error::{
        ClientEndpointError, ConnectionError, EndpointError, RecvError, RpcError,
        SerializationError,
    },
};
use bytes::Bytes;
use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use tokio::sync::broadcast::{self, Sender};
use tokio::sync::mpsc::{self, Receiver as MpscReceiver, Sender as MpscSender};
use tokio::time::{timeout, Duration};
use tracing::{debug, error, info, trace, warn};

// Number of seconds before timing out the IGD request to forward a port.
#[cfg(feature = "igd")]
const PORT_FORWARD_TIMEOUT: Duration = Duration::from_secs(30);

// Number of seconds before timing out the echo service query.
const ECHO_SERVICE_QUERY_TIMEOUT: Duration = Duration::from_secs(30);

/// Standard size of our channel bounds
const STANDARD_CHANNEL_SIZE: usize = 10000;

/// Channel on which incoming messages can be listened to
#[derive(Debug)]
pub struct IncomingMessages<I: ConnId>(pub(crate) MpscReceiver<(Connection<I>, Bytes)>);

impl<I: ConnId> IncomingMessages<I> {
    /// Blocks and returns the next incoming message and the connection it arrived on.
    ///
    /// **Note:** holding on to `Connection`
    pub async fn next(&mut self) -> Option<(Connection<I>, Bytes)> {
        self.0.recv().await
    }
}

/// Channel on which incoming connections are notified on
pub struct IncomingConnections<I: ConnId>(pub(crate) MpscReceiver<Connection<I>>);

impl<I: ConnId> IncomingConnections<I> {
    /// Blocks until there is an incoming connection and returns the address of the
    /// connecting peer
    pub async fn next(&mut self) -> Option<Connection<I>> {
        self.0.recv().await
    }
}

/// Endpoint instance which can be used to communicate with peers.
#[derive(Clone)]
pub struct Endpoint<I: ConnId> {
    local_addr: SocketAddr,
    public_addr: Option<SocketAddr>,
    quic_endpoint: quinn::Endpoint,
    message_tx: MpscSender<(Connection<I>, Bytes)>,
    disconnection_tx: MpscSender<SocketAddr>,
    config: InternalConfig,
    termination_tx: Sender<()>,
    connection_pool: ConnectionPool<I>,
    connection_deduplicator: ConnectionDeduplicator,

    // counts fully opened connections, excluding incoming and connections dropped by connect_to_any
    opened_connection_count: Arc<AtomicUsize>,
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
    /// Create a peer endpoint at the given address.
    ///
    /// A peer endpoint, unlike a [client](Self::new_client) endpoint, can receive incoming
    /// connections.
    ///
    /// # Bootstrapping
    ///
    /// When given a non-empty list of `contacts`, this will attempt to 'bootstrap' against them.
    /// This involves connecting to all the contacts concurrently and selecting the first
    /// successfully connected peer (if any), whose `SocketAddr` will be returned.
    ///
    /// If bootstrapping is successful, the connected peer will be used to perform a reachability
    /// check to validate that this endpoint can be reached at its
    /// [`public_addr`](Self::public_addr).
    ///
    /// **Note:** if no contacts are given, the [`public_addr`](Self::public_addr) of the endpoint
    /// will not have been validated to be reachable by anyone
    ///
    /// # Port forwarding (UPnP)
    ///
    /// If configured (via `config.forward_port`), an external port mapping will be set up (using
    /// the IGD UPnP protocol). The established external port will be reflected in
    /// [`public_addr`](Self::public_addr), and the lease will be renewed automatically every
    /// `config.upnp_lease_duration`.
    pub async fn new(
        local_addr: impl Into<SocketAddr>,
        contacts: &[SocketAddr],
        config: Config,
    ) -> Result<
        (
            Self,
            IncomingConnections<I>,
            IncomingMessages<I>,
            DisconnectionEvents,
            Option<Connection<I>>,
        ),
        EndpointError,
    > {
        let config = InternalConfig::try_from_config(config)?;

        let mut builder = quinn::Endpoint::builder();
        let _ = builder.listen(config.server.clone());

        let (mut endpoint, quic_incoming, channels) =
            Self::build_endpoint(local_addr.into(), config, builder)?;

        let contact = endpoint.connect_to_any(contacts).await;
        let public_addr = endpoint.resolve_public_addr(contact.as_ref()).await?;

        #[cfg(feature = "igd")]
        if endpoint.config.forward_port {
            timeout(
                PORT_FORWARD_TIMEOUT,
                forward_port(
                    public_addr.port(),
                    endpoint.local_addr(),
                    endpoint.config.upnp_lease_duration,
                    channels.termination.1,
                ),
            )
            .await
            .map_err(|_| IgdError::TimedOut)??;
        }

        listen_for_incoming_connections(
            quic_incoming,
            endpoint.connection_pool.clone(),
            channels.message.0.clone(),
            channels.connection.0,
            channels.disconnection.0.clone(),
            endpoint.clone(),
        );

        if let Some(contact) = contact.as_ref() {
            let valid = endpoint
                .endpoint_verification(contact, public_addr)
                .await
                .map_err(|error| EndpointError::EndpointVerification {
                    peer: contact.remote_address(),
                    error,
                })?;
            if !valid {
                return Err(EndpointError::Unreachable { public_addr });
            }
        }

        Ok((
            endpoint,
            IncomingConnections(channels.connection.1),
            IncomingMessages(channels.message.1),
            DisconnectionEvents(channels.disconnection.1),
            contact,
        ))
    }

    /// Create a client endpoint at the given address.
    ///
    /// A client endpoint cannot receive incoming connections, as such they also do not need to be
    /// publicly reachable. They can still communicate over outgoing connections and receive
    /// incoming streams, since QUIC allows for either side of a connection to initiate streams.
    pub fn new_client(
        local_addr: impl Into<SocketAddr>,
        config: Config,
    ) -> Result<(Self, IncomingMessages<I>, DisconnectionEvents), ClientEndpointError> {
        let config = InternalConfig::try_from_config(config)?;

        let (endpoint, _, channels) =
            Self::build_endpoint(local_addr.into(), config, quinn::Endpoint::builder())?;

        Ok((
            endpoint,
            IncomingMessages(channels.message.1),
            DisconnectionEvents(channels.disconnection.1),
        ))
    }

    // A private helper for initialising an endpoint.
    fn build_endpoint(
        local_addr: SocketAddr,
        config: InternalConfig,
        builder: quinn::EndpointBuilder,
    ) -> Result<(Self, quinn::Incoming, Channels<I>), quinn::EndpointError> {
        let (quic_endpoint, quic_incoming) = builder.bind(&local_addr)?;
        let local_addr = quic_endpoint
            .local_addr()
            .map_err(quinn::EndpointError::Socket)?;

        let channels = Channels::new();

        let endpoint = Self {
            local_addr,
            public_addr: None,
            quic_endpoint,
            message_tx: channels.message.0.clone(),
            disconnection_tx: channels.disconnection.0.clone(),
            config,
            termination_tx: channels.termination.0.clone(),
            connection_pool: ConnectionPool::new(),
            connection_deduplicator: ConnectionDeduplicator::new(),
            opened_connection_count: Arc::new(AtomicUsize::new(0)),
        };

        Ok((endpoint, quic_incoming, channels))
    }

    /// Endpoint local address
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Get the public address of the endpoint.
    pub fn public_addr(&self) -> SocketAddr {
        self.public_addr.unwrap_or(self.local_addr)
    }

    /// Get the count of opened connections.
    ///
    /// `Endpoint`s keep a count of connections they have fully opened and returned to callers.
    /// Notably this excludes any incoming connections, and connections that were dropped by
    /// [`connect_to_any`](Self::connect_to_any).
    #[stability::unstable(feature = "opened-connection-count")]
    pub fn opened_connection_count(&self) -> usize {
        self.opened_connection_count.load(Ordering::Relaxed)
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

    /// Connect to a peer.
    ///
    /// Atttempts to connect to a peer at the given address. Connection attempts are retried based
    /// on the [`Config::retry_config`] used to create the endpoint.
    ///
    /// Returns a [`Connection`], which is a handle representing the underlying connection. There's
    /// presently little that you can do with a `Connection` itself, besides obtaining the
    /// [`remote_address`](Connection::remote_address) which can then be used with other `Endpoint`
    /// methods to communicate with the peer (the connection is retrieved via the connection pool –
    /// see below).
    ///
    /// **Note:** this method is intended for use when it's necessary to connect to a specific peer.
    /// See [`connect_to_any`](Self::connect_to_any) if you just need a connection with any of a set
    /// of peers.
    ///
    /// # Connection pooling
    ///
    /// Connections are stored in an internal pool and reused if possible. A connection remains in
    /// the pool until either side closes the connection (including due to timeouts or errors). This
    /// method will check the pool before opening a new connection. If a new connection is opened,
    /// it will be added to the pool.
    pub async fn connect_to(
        &self,
        node_addr: &SocketAddr,
    ) -> Result<Connection<I>, ConnectionError> {
        self.get_or_connect_to(node_addr).await
    }

    /// Connect to any of the given peers.
    ///
    /// Often in peer-to-peer networks, it's sufficient to communicate to any node on the network,
    /// rather than having to connect to specific nodes. This method will start connecting to every
    /// peer in `peer_addrs`, and return the address of the first successfully established
    /// connection (the rest are cancelled and discarded). All connection attempts will be retried
    /// based on the [`Config::retry_config`] used to create the endpoint.
    ///
    /// # Connection pooling
    ///
    /// Connections are stored in an internal pool and reused if possible. A connection remains in
    /// the pool until either side closes the connection (including due to timeouts or errors). This
    /// method will check the pool before opening each connection. If a new connection is opened, it
    /// will be added to the pool. Note that already pooled connections will have a higher chance of
    /// 'winning' the race, and being the selected peer.
    pub async fn connect_to_any(&self, peer_addrs: &[SocketAddr]) -> Option<Connection<I>> {
        trace!("Connecting to any of {:?}", peer_addrs);
        if peer_addrs.is_empty() {
            return None;
        }

        // Attempt to create a new connection to all nodes and return the first one to succeed
        let tasks = peer_addrs
            .iter()
            .map(|addr| Box::pin(self.get_or_connect_to(addr)));

        match futures::future::select_ok(tasks).await {
            Ok((connection, _)) => Some(connection),
            Err(error) => {
                error!("Failed to bootstrap to the network, last error: {}", error);
                None
            }
        }
    }

    /// Verify if an address is publicly reachable. This will attempt to create
    /// a new connection and use it to exchange a message and verify that the node
    /// can be reached.
    ///
    /// # Connection pooling
    ///
    /// Note that unlike most methods on `Endpoint`, this **will not** use the connection pool. This
    /// ensures that the reachability check is accurate, otherwise we may use a pooled connection
    /// that was opened by the peer (which tells us nothing about whether we can open a connection
    /// to the peer).
    pub async fn is_reachable(&self, peer_addr: &SocketAddr) -> Result<(), RpcError> {
        trace!("Checking is reachable");

        // avoid the connection pool
        let quinn::NewConnection { connection, .. } = self.new_connection(peer_addr).await?;
        let (send_stream, recv_stream) = connection.open_bi().await?;
        let mut send_stream = SendStream::new(send_stream);
        let mut recv_stream = RecvStream::new(recv_stream);

        send_stream.send(WireMsg::EndpointEchoReq).await?;

        match timeout(ECHO_SERVICE_QUERY_TIMEOUT, recv_stream.next_wire_msg()).await?? {
            Some(WireMsg::EndpointEchoResp(_)) => Ok(()),
            Some(other) => {
                info!(
                    "Unexpected message type when verifying reachability: {}",
                    &other
                );
                Ok(())
            }
            None => {
                info!(
                    "Peer {} did not reply when verifying reachability",
                    peer_addr
                );
                Ok(())
            }
        }
    }

    /// Get the existing `Connection` for a `SocketAddr`.
    pub async fn get_connection_by_addr(&self, addr: &SocketAddr) -> Option<Connection<I>> {
        self.connection_pool
            .get_by_addr(addr)
            .await
            .map(|(connection, remover)| self.wrap_connection(connection, remover))
    }

    /// Get the existing `Connection` for the given ID.
    pub async fn get_connection_by_id(&self, id: &I) -> Option<Connection<I>> {
        self.connection_pool
            .get_by_id(id)
            .await
            .map(|(connection, remover)| self.wrap_connection(connection, remover))
    }

    /// Open a bi-directional stream with a given peer.
    ///
    /// # Priority
    ///
    /// Locally buffered data from streams with higher priority will be transmitted before data from
    /// streams with lower priority. Changing the priority of a stream with pending data may only
    /// take effect after that data has been transmitted. Using many different priority levels per
    /// connection may have a negative impact on performance.
    ///
    /// `0` is a sensible default for 'normal' priority.
    ///
    /// # Connection pooling
    ///
    /// Connections are stored in an internal pool and reused if possible. A connection remains in
    /// the pool until either side closes the connection (including due to timeouts or errors). This
    /// method will check the pool before opening a new connection. If a new connection is opened,
    /// it will be added to the pool.
    pub async fn open_bidirectional_stream(
        &self,
        peer_addr: &SocketAddr,
        priority: i32,
    ) -> Result<(SendStream, RecvStream), ConnectionError> {
        let connection = self.get_or_connect_to(peer_addr).await?;
        connection.open_bi(priority).await
    }

    /// Close all the connections of this endpoint immediately and stop accepting new connections.
    pub fn close(&self) {
        let _ = self.termination_tx.send(());
        self.quic_endpoint.close(0_u32.into(), b"")
    }

    /// Get a connection from the pool, or create one, for the given `addr`.
    pub(crate) async fn get_or_connect_to(
        &self,
        addr: &SocketAddr,
    ) -> Result<Connection<I>, ConnectionError> {
        let completion = loop {
            if let Some((conn, remover)) = self.connection_pool.get_by_addr(addr).await {
                trace!("We are already connected to this peer: {}", addr);
                return Ok(self.wrap_connection(conn, remover));
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
                let connection = self.wrap_connection(connection, remover);

                listen_for_incoming_messages(
                    connection.clone(),
                    new_connection.uni_streams,
                    new_connection.bi_streams,
                    self.message_tx.clone(),
                    self.disconnection_tx.clone(),
                    self.clone(),
                );

                let _ = completion.complete(Ok(()));

                let _ = self.opened_connection_count.fetch_add(1, Ordering::Relaxed);

                Ok(connection)
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
        self.config
            .retry_config
            .retry(|| async {
                trace!("Attempting to connect to {:?}", node_addr);
                let connecting = match self.quic_endpoint.connect_with(
                    self.config.client.clone(),
                    node_addr,
                    SERVER_NAME,
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

    // set an appropriate public address based on `config` and a reachability check.
    async fn resolve_public_addr(
        &mut self,
        contact: Option<&Connection<I>>,
    ) -> Result<SocketAddr, EndpointError> {
        let mut public_addr = self.local_addr;

        // get the IP seen for us by our contact
        let visible_addr = if let Some(contact) = contact {
            Some(self.endpoint_echo(contact).await.map_err(|error| {
                EndpointError::EndpointEcho {
                    peer: contact.remote_address(),
                    error,
                }
            })?)
        } else {
            None
        };

        if let Some(external_ip) = self.config.external_ip {
            // set the public IP based on config
            public_addr.set_ip(external_ip);

            if let Some(visible_addr) = visible_addr {
                // if we set a different external IP than peers can see, we will have a bad time
                if visible_addr.ip() != external_ip {
                    warn!(
                        "Configured external IP ({}) does not match that seen by peers ({})",
                        external_ip,
                        visible_addr.ip()
                    );
                }
            }
        } else if let Some(visible_addr) = visible_addr {
            // set the public IP based on that seen by the peer
            public_addr.set_ip(visible_addr.ip());
        } else {
            // we have no good source for public IP, leave it as the local IP and warn
            warn!(
                "Could not determine better public IP than local IP ({})",
                public_addr.ip()
            );
        }

        if let Some(external_port) = self.config.external_port {
            // set the public port based on config
            public_addr.set_port(external_port);

            if let Some(visible_addr) = visible_addr {
                // if we set a different external IP than peers can see, we will have a bad time
                if visible_addr.port() != external_port {
                    warn!(
                        "Configured external port ({}) does not match that seen by peers ({})",
                        external_port,
                        visible_addr.port()
                    );
                }
            }
        } else if let Some(visible_addr) = visible_addr {
            // set the public port based on that seen by the peer
            public_addr.set_port(visible_addr.port());
        } else {
            // we have no good source for public port, leave it as the local port and warn
            warn!(
                "Could not determine better public port than local port ({})",
                public_addr.port()
            );
        }

        self.public_addr = Some(public_addr);

        // Return the address so callers can avoid the optionality of `self.public_addr`
        Ok(public_addr)
    }

    /// Perform the endpoint echo RPC with the given contact.
    async fn endpoint_echo(&self, contact: &Connection<I>) -> Result<SocketAddr, RpcError> {
        let (mut send, mut recv) = contact.open_bi(0).await?;

        send.send(WireMsg::EndpointEchoReq).await?;

        match timeout(ECHO_SERVICE_QUERY_TIMEOUT, recv.next_wire_msg()).await?? {
            Some(WireMsg::EndpointEchoResp(addr)) => Ok(addr),
            msg => Err(RecvError::Serialization(SerializationError::unexpected(msg)).into()),
        }
    }

    /// Perform the endpoint verification RPC with the given peer.
    async fn endpoint_verification(
        &self,
        contact: &Connection<I>,
        public_addr: SocketAddr,
    ) -> Result<bool, RpcError> {
        let (mut send, mut recv) = contact.open_bi(0).await?;

        send.send(WireMsg::EndpointVerificationReq(public_addr))
            .await?;

        match timeout(ECHO_SERVICE_QUERY_TIMEOUT, recv.next_wire_msg()).await?? {
            Some(WireMsg::EndpointVerificationResp(valid)) => Ok(valid),
            msg => Err(RecvError::Serialization(SerializationError::unexpected(msg)).into()),
        }
    }

    /// Wrap a quinn connection, setting the default retry config and pool remover.
    pub(crate) fn wrap_connection(
        &self,
        connection: quinn::Connection,
        remover: ConnectionRemover<I>,
    ) -> Connection<I> {
        Connection::new(connection, self.config.retry_config.clone(), remover)
    }
}

// a private helper struct for passing a bunch of channel-related things
type Msg<I> = (Connection<I>, Bytes);
struct Channels<I: ConnId> {
    connection: (MpscSender<Connection<I>>, MpscReceiver<Connection<I>>),
    message: (MpscSender<Msg<I>>, MpscReceiver<Msg<I>>),
    disconnection: (MpscSender<SocketAddr>, MpscReceiver<SocketAddr>),
    termination: (Sender<()>, broadcast::Receiver<()>),
}

impl<I: ConnId> Channels<I> {
    fn new() -> Self {
        Self {
            connection: mpsc::channel(STANDARD_CHANNEL_SIZE),
            message: mpsc::channel(STANDARD_CHANNEL_SIZE),
            disconnection: mpsc::channel(STANDARD_CHANNEL_SIZE),
            termination: broadcast::channel(1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Endpoint;
    use crate::{tests::local_addr, Config};
    use color_eyre::eyre::Result;
    use std::net::SocketAddr;

    #[tokio::test]
    async fn new_without_external_addr() -> Result<()> {
        let (endpoint, _, _, _, _) = Endpoint::<[u8; 32]>::new(
            local_addr(),
            &[],
            Config {
                external_ip: None,
                external_port: None,
                ..Default::default()
            },
        )
        .await?;
        assert_eq!(endpoint.public_addr(), endpoint.local_addr());

        Ok(())
    }

    #[tokio::test]
    async fn new_with_external_ip() -> Result<()> {
        let (endpoint, _, _, _, _) = Endpoint::<[u8; 32]>::new(
            local_addr(),
            &[],
            Config {
                external_ip: Some([123u8, 123, 123, 123].into()),
                external_port: None,
                ..Default::default()
            },
        )
        .await?;
        assert_eq!(
            endpoint.public_addr(),
            SocketAddr::new([123u8, 123, 123, 123].into(), endpoint.local_addr().port())
        );

        Ok(())
    }

    #[tokio::test]
    async fn new_with_external_port() -> Result<()> {
        let (endpoint, _, _, _, _) = Endpoint::<[u8; 32]>::new(
            local_addr(),
            &[],
            Config {
                external_ip: None,
                external_port: Some(123),
                ..Default::default()
            },
        )
        .await?;
        assert_eq!(
            endpoint.public_addr(),
            SocketAddr::new(endpoint.local_addr().ip(), 123)
        );

        Ok(())
    }

    #[tokio::test]
    async fn new_with_external_addr() -> Result<()> {
        let (endpoint, _, _, _, _) = Endpoint::<[u8; 32]>::new(
            local_addr(),
            &[],
            Config {
                external_ip: Some([123u8, 123, 123, 123].into()),
                external_port: Some(123),
                ..Default::default()
            },
        )
        .await?;
        assert_eq!(endpoint.public_addr(), "123.123.123.123:123".parse()?);

        Ok(())
    }
}
