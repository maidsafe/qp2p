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
    config::{Config, InternalConfig, RetryConfig},
    connections::{
        listen_for_incoming_connections, listen_for_incoming_messages, Connection,
        DisconnectionEvents, RecvStream, SendStream,
    },
    error::{
        ClientEndpointError, ConnectionError, EndpointError, RecvError, RpcError,
        SerializationError,
    },
};
use backoff::{future::retry, ExponentialBackoff};
use bytes::Bytes;
use std::net::SocketAddr;
use tokio::sync::broadcast::{self, Sender};
use tokio::sync::mpsc::{self, Receiver as MpscReceiver, Sender as MpscSender};
use tokio::time::{timeout, Duration};
use tracing::{debug, error, info, trace, warn};

/// Host name of the Quic communication certificate used by peers
// FIXME: make it configurable
const CERT_SERVER_NAME: &str = "MaidSAFE.net";

// Number of seconds before timing out the IGD request to forward a port.
#[cfg(feature = "igd")]
const PORT_FORWARD_TIMEOUT: Duration = Duration::from_secs(30);

// Number of seconds before timing out the echo service query.
const ECHO_SERVICE_QUERY_TIMEOUT: Duration = Duration::from_secs(30);

/// Standard size of our channel bounds
const STANDARD_CHANNEL_SIZE: usize = 10000;

/// Channel on which incoming messages can be listened to
#[derive(Debug)]
pub struct IncomingMessages(pub(crate) MpscReceiver<(Connection, Bytes)>);

impl IncomingMessages {
    /// Blocks and returns the next incoming message and the source peer address
    pub async fn next(&mut self) -> Option<(Connection, Bytes)> {
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
pub struct Endpoint {
    local_addr: SocketAddr,
    public_addr: Option<SocketAddr>,
    quic_endpoint: quinn::Endpoint,
    message_tx: MpscSender<(Connection, Bytes)>,
    disconnection_tx: MpscSender<SocketAddr>,
    config: InternalConfig,
    termination_tx: Sender<()>,
}

impl std::fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Endpoint")
            .field("local_addr", &self.local_addr)
            .field("quic_endpoint", &"<endpoint omitted>")
            .field("config", &self.config)
            .finish()
    }
}

impl Endpoint {
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
            IncomingConnections,
            IncomingMessages,
            DisconnectionEvents,
            Option<Connection>,
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
            channels.message.0.clone(),
            channels.connection.0,
            channels.disconnection.0.clone(),
            endpoint.clone(),
        );

        if let Some(peer) = &contact {
            let valid = endpoint
                .endpoint_verification(peer, public_addr)
                .await
                .map_err(|error| EndpointError::EndpointVerification {
                    peer: peer.remote_address(),
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
    ) -> Result<(Self, IncomingMessages, DisconnectionEvents), ClientEndpointError> {
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
    ) -> Result<(Self, quinn::Incoming, Channels), quinn::EndpointError> {
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
    pub async fn connect_to(&self, node_addr: &SocketAddr) -> Result<Connection, ConnectionError> {
        let new_connection = self.new_connection(node_addr).await?;

        trace!("Successfully connected to peer: {}", node_addr);

        let connection = Connection::new(new_connection.connection);

        listen_for_incoming_messages(
            connection.remote_address(),
            connection.clone(),
            new_connection.uni_streams,
            new_connection.bi_streams,
            self.message_tx.clone(),
            self.disconnection_tx.clone(),
            self.clone(),
        );

        Ok(connection)
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
    pub async fn connect_to_any(&self, peer_addrs: &[SocketAddr]) -> Option<Connection> {
        trace!("Connecting to any of {:?}", peer_addrs);
        if peer_addrs.is_empty() {
            return None;
        }

        // Attempt to create a new connection to all nodes and return the first one to succeed
        let tasks = peer_addrs
            .iter()
            .map(|addr| Box::pin(self.connect_to(addr)));

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

    /// Close all the connections of this endpoint immediately and stop accepting new connections.
    pub fn close(&self) {
        let _ = self.termination_tx.send(());
        self.quic_endpoint.close(0_u32.into(), b"")
    }

    /// Attempt a connection to a node_addr.
    ///
    /// All failures are retried with exponential back-off. This doesn't use the connection pool, it
    /// will always try to open a new connection.
    async fn new_connection(
        &self,
        node_addr: &SocketAddr,
    ) -> Result<quinn::NewConnection, ConnectionError> {
        Self::retry(self.config.retry_config, || async {
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

    // set an appropriate public address based on `config` and a reachability check.
    async fn resolve_public_addr(
        &mut self,
        contact: Option<&Connection>,
    ) -> Result<SocketAddr, EndpointError> {
        let mut public_addr = self.local_addr;

        // get the IP seen for us by our contact
        let visible_addr =
            if let Some(peer) = contact {
                Some(self.endpoint_echo(peer).await.map_err(|error| {
                    EndpointError::EndpointEcho {
                        peer: peer.remote_address(),
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

    /// Perform the endpoint echo RPC with the given peer.
    async fn endpoint_echo(&self, peer: &Connection) -> Result<SocketAddr, RpcError> {
        let (mut send, mut recv) = peer.open_bi(0).await?;

        send.send(WireMsg::EndpointEchoReq).await?;

        match timeout(ECHO_SERVICE_QUERY_TIMEOUT, recv.next_wire_msg()).await?? {
            Some(WireMsg::EndpointEchoResp(addr)) => Ok(addr),
            msg => Err(RecvError::Serialization(SerializationError::unexpected(msg)).into()),
        }
    }

    /// Perform the endpoint verification RPC with the given peer.
    async fn endpoint_verification(
        &self,
        peer: &Connection,
        public_addr: SocketAddr,
    ) -> Result<bool, RpcError> {
        let (mut send, mut recv) = peer.open_bi(0).await?;

        send.send(WireMsg::EndpointVerificationReq(public_addr))
            .await?;

        match timeout(ECHO_SERVICE_QUERY_TIMEOUT, recv.next_wire_msg()).await?? {
            Some(WireMsg::EndpointVerificationResp(valid)) => Ok(valid),
            msg => Err(RecvError::Serialization(SerializationError::unexpected(msg)).into()),
        }
    }

    fn retry<R, E, Fn, Fut>(cfg: RetryConfig, op: Fn) -> impl futures::Future<Output = Result<R, E>>
    where
        Fn: FnMut() -> Fut,
        Fut: futures::Future<Output = Result<R, backoff::Error<E>>>,
    {
        let backoff = ExponentialBackoff {
            initial_interval: cfg.initial_retry_interval,
            randomization_factor: cfg.retry_delay_rand_factor,
            multiplier: cfg.retry_delay_multiplier,
            max_interval: cfg.max_retry_interval,
            max_elapsed_time: Some(cfg.retrying_max_elapsed_time),
            ..Default::default()
        };
        retry(backoff, op)
    }
}

// a private helper struct for passing a bunch of channel-related things
type Msg = (Connection, Bytes);
struct Channels {
    connection: (MpscSender<SocketAddr>, MpscReceiver<SocketAddr>),
    message: (MpscSender<Msg>, MpscReceiver<Msg>),
    disconnection: (MpscSender<SocketAddr>, MpscReceiver<SocketAddr>),
    termination: (Sender<()>, broadcast::Receiver<()>),
}

impl Channels {
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
        let (endpoint, _, _, _, _) = Endpoint::new(
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
        let (endpoint, _, _, _, _) = Endpoint::new(
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
        let (endpoint, _, _, _, _) = Endpoint::new(
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
        let (endpoint, _, _, _, _) = Endpoint::new(
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
