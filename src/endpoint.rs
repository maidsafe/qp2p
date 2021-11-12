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
    config::{Config, InternalConfig, RetryConfig, SERVER_NAME},
    connection::{Connection, ConnectionIncoming},
    error::{
        ClientEndpointError, ConnectionError, EndpointError, RecvError, RpcError,
        SerializationError,
    },
};
use backoff::backoff::Backoff;
use futures::StreamExt;
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
};
use tokio::sync::broadcast::{self, Sender};
use tokio::sync::mpsc::{self, Receiver as MpscReceiver};
use tokio::time::{timeout, Duration};
use tracing::{error, info, trace, warn};

// Number of seconds before timing out the IGD request to forward a port.
#[cfg(feature = "igd")]
const PORT_FORWARD_TIMEOUT: Duration = Duration::from_secs(30);

// Number of seconds before timing out the echo service query.
const ECHO_SERVICE_QUERY_TIMEOUT: Duration = Duration::from_secs(30);

/// Standard size of our channel bounds
const STANDARD_CHANNEL_SIZE: usize = 10000;

/// Channel on which incoming connections are notified on
pub struct IncomingConnections(pub(crate) MpscReceiver<(Connection, ConnectionIncoming)>);

impl IncomingConnections {
    /// Blocks until there is an incoming connection and returns the address of the
    /// connecting peer
    pub async fn next(&mut self) -> Option<(Connection, ConnectionIncoming)> {
        self.0.recv().await
    }
}

/// Endpoint instance which can be used to communicate with peers.
#[derive(Clone)]
pub struct Endpoint {
    local_addr: SocketAddr,
    public_addr: Option<SocketAddr>,
    quic_endpoint: quinn::Endpoint,
    default_retry_config: Arc<RetryConfig>,

    termination_tx: Sender<()>,
}

impl std::fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Endpoint")
            .field("local_addr", &self.local_addr)
            .field("quic_endpoint", &"<endpoint omitted>")
            .field("default_retry_config", &self.default_retry_config)
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
            Option<(Connection, ConnectionIncoming)>,
        ),
        EndpointError,
    > {
        let config = InternalConfig::try_from_config(config)?;

        let mut builder = quinn::Endpoint::builder();
        let _ = builder.listen(config.server.clone());

        let (mut endpoint, quic_incoming, termination_rx) = Self::build_endpoint(
            local_addr.into(),
            config.client,
            config.default_retry_config,
            builder,
        )?;

        let contact = endpoint.connect_to_any(contacts).await;
        let public_addr = endpoint
            .resolve_public_addr(
                config.external_ip,
                config.external_port,
                contact.as_ref().map(|c| &c.0),
            )
            .await?;

        #[cfg(feature = "igd")]
        if config.forward_port {
            timeout(
                PORT_FORWARD_TIMEOUT,
                forward_port(
                    public_addr.port(),
                    endpoint.local_addr(),
                    config.upnp_lease_duration,
                    termination_rx,
                ),
            )
            .await
            .map_err(|_| IgdError::TimedOut)??;
        }

        #[cfg(not(feature = "igd"))]
        drop(termination_rx); // not needed if igd is disabled

        let (connection_tx, connection_rx) = mpsc::channel(STANDARD_CHANNEL_SIZE);
        listen_for_incoming_connections(
            quic_incoming,
            connection_tx,
            endpoint.quic_endpoint.clone(),
            endpoint.default_retry_config.clone(),
        );

        if let Some((contact, _)) = contact.as_ref() {
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

        Ok((endpoint, IncomingConnections(connection_rx), contact))
    }

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

        let (endpoint, _, _) = Self::build_endpoint(
            local_addr.into(),
            config.client,
            config.default_retry_config,
            quinn::Endpoint::builder(),
        )?;

        Ok(endpoint)
    }

    // A private helper for initialising an endpoint.
    fn build_endpoint(
        local_addr: SocketAddr,
        client_config: quinn::ClientConfig,
        default_retry_config: Arc<RetryConfig>,
        mut builder: quinn::EndpointBuilder,
    ) -> Result<(Self, quinn::Incoming, broadcast::Receiver<()>), quinn::EndpointError> {
        let _ = builder.default_client_config(client_config);
        let (quic_endpoint, quic_incoming) = builder.bind(&local_addr)?;
        let local_addr = quic_endpoint
            .local_addr()
            .map_err(quinn::EndpointError::Socket)?;

        let (termination_tx, termination_rx) = broadcast::channel(1);

        let endpoint = Self {
            local_addr,
            public_addr: None,
            quic_endpoint,
            default_retry_config,
            termination_tx,
        };

        Ok((endpoint, quic_incoming, termination_rx))
    }

    /// Endpoint local address
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Get the public address of the endpoint.
    pub fn public_addr(&self) -> SocketAddr {
        self.public_addr.unwrap_or(self.local_addr)
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
    ) -> Result<(Connection, ConnectionIncoming), ConnectionError> {
        self.new_connection(node_addr, self.default_retry_config.backoff())
            .await
    }

    /// Connect to a peer with a given retry context.
    ///
    /// [`Endpoint::connect_to`] will retry connection attempts based on the configured
    /// `default_retry_config`. This method allows the caller to specify a [`Backoff`]
    /// implementation to use instead. In particular, this would allow a single retry 'context' to
    /// be maintained through multiple calls.
    ///
    /// See [`Endpoint::connect_to`] for more information.
    pub async fn connect_with_retries(
        &self,
        node_addr: &SocketAddr,
        backoff: impl Backoff,
    ) -> Result<(Connection, ConnectionIncoming), ConnectionError> {
        self.new_connection(node_addr, backoff).await
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
    pub async fn connect_to_any(
        &self,
        peer_addrs: &[SocketAddr],
    ) -> Option<(Connection, ConnectionIncoming)> {
        trace!("Connecting to any of {:?}", peer_addrs);
        if peer_addrs.is_empty() {
            return None;
        }

        // Attempt to create a new connection to all nodes and return the first one to succeed
        let tasks = peer_addrs
            .iter()
            .map(|addr| Box::pin(self.new_connection(addr, self.default_retry_config.backoff())));

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

        let (connection, _) = self
            .new_connection(peer_addr, self.default_retry_config.backoff())
            .await?;
        let (mut send_stream, mut recv_stream) = connection.open_bi().await?;

        send_stream.send_wire_msg(WireMsg::EndpointEchoReq).await?;

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
        backoff: impl Backoff,
    ) -> Result<(Connection, ConnectionIncoming), ConnectionError> {
        let operation = || async {
            trace!("Attempting to connect to {:?}", node_addr);
            let connecting = match self.quic_endpoint.connect(node_addr, SERVER_NAME) {
                Ok(conn) => Ok(conn),
                Err(error) => {
                    warn!("Connection attempt failed due to {:?}", error);
                    Err(ConnectionError::from(error))
                }
            }?;

            let new_conn = match connecting.await {
                Ok(new_conn) => {
                    trace!("Successfully connected to peer: {}", node_addr);

                    let (connection, connection_incoming) = Connection::new(
                        self.quic_endpoint.clone(),
                        Some(self.default_retry_config.clone()),
                        new_conn,
                    );

                    Ok((connection, connection_incoming))
                }
                Err(error) => Err(ConnectionError::from(error)),
            }?;

            Ok(new_conn)
        };

        backoff::future::retry(backoff, operation).await
    }

    // set an appropriate public address based on `config` and a reachability check.
    async fn resolve_public_addr(
        &mut self,
        config_external_ip: Option<IpAddr>,
        config_external_port: Option<u16>,
        contact: Option<&Connection>,
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

        if let Some(external_ip) = config_external_ip {
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

        if let Some(external_port) = config_external_port {
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
    async fn endpoint_echo(&self, contact: &Connection) -> Result<SocketAddr, RpcError> {
        let (mut send, mut recv) = contact.open_bi().await?;

        send.send_wire_msg(WireMsg::EndpointEchoReq).await?;

        match timeout(ECHO_SERVICE_QUERY_TIMEOUT, recv.next_wire_msg()).await?? {
            Some(WireMsg::EndpointEchoResp(addr)) => Ok(addr),
            msg => Err(RecvError::Serialization(SerializationError::unexpected(msg)).into()),
        }
    }

    /// Perform the endpoint verification RPC with the given peer.
    async fn endpoint_verification(
        &self,
        contact: &Connection,
        public_addr: SocketAddr,
    ) -> Result<bool, RpcError> {
        let (mut send, mut recv) = contact.open_bi().await?;

        send.send_wire_msg(WireMsg::EndpointVerificationReq(public_addr))
            .await?;

        match timeout(ECHO_SERVICE_QUERY_TIMEOUT, recv.next_wire_msg()).await?? {
            Some(WireMsg::EndpointVerificationResp(valid)) => Ok(valid),
            msg => Err(RecvError::Serialization(SerializationError::unexpected(msg)).into()),
        }
    }
}

pub(super) fn listen_for_incoming_connections(
    mut quinn_incoming: quinn::Incoming,
    connection_tx: mpsc::Sender<(Connection, ConnectionIncoming)>,
    quic_endpoint: quinn::Endpoint,
    retry_config: Arc<RetryConfig>,
) {
    let _ = tokio::spawn(async move {
        loop {
            match quinn_incoming.next().await {
                Some(quinn_conn) => match quinn_conn.await {
                    Ok(connection) => {
                        let (connection, connection_incoming) = Connection::new(
                            quic_endpoint.clone(),
                            Some(retry_config.clone()),
                            connection,
                        );

                        if connection_tx
                            .send((connection, connection_incoming))
                            .await
                            .is_err()
                        {
                            warn!("Dropping incoming connection because receiver was dropped");
                        }
                    }
                    Err(err) => {
                        warn!("An incoming connection failed because of: {:?}", err);
                    }
                },
                None => {
                    trace!("quinn::Incoming::next() returned None. There will be no more incoming connections");
                    break;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::Endpoint;
    use crate::{tests::local_addr, Config};
    use color_eyre::eyre::Result;
    use std::net::SocketAddr;

    #[tokio::test]
    async fn new_without_external_addr() -> Result<()> {
        let (endpoint, _, _) = Endpoint::new(
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
        let (endpoint, _, _) = Endpoint::new(
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
        let (endpoint, _, _) = Endpoint::new(
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
        let (endpoint, _, _) = Endpoint::new(
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
