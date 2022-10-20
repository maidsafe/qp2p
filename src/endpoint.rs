// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use super::wire_msg::WireMsg;
use super::{
    config::{Config, InternalConfig, SERVER_NAME},
    connection::{Connection, ConnectionIncoming},
    error::{ClientEndpointError, ConnectionError, EndpointError, RpcError},
};
use quinn::Endpoint as QuinnEndpoint;
use std::net::{IpAddr, SocketAddr};
use tokio::sync::broadcast::{self, Sender};
use tokio::sync::mpsc::{self, error::TryRecvError, Receiver as MpscReceiver};
use tokio::time::{timeout, Duration};
use tracing::{error, info, trace, warn};

// Number of seconds before timing out the echo service query.
const ECHO_SERVICE_QUERY_TIMEOUT: Duration = Duration::from_secs(30);

/// Standard size of our channel bounds
const STANDARD_CHANNEL_SIZE: usize = 10000;

/// Channel on which incoming connections are notified on
#[derive(Debug)]
pub struct IncomingConnections(pub(crate) MpscReceiver<(Connection, ConnectionIncoming)>);

impl IncomingConnections {
    /// Blocks until there is an incoming connection and returns the address of the
    /// connecting peer
    pub async fn next(&mut self) -> Option<(Connection, ConnectionIncoming)> {
        self.0.recv().await
    }

    /// Non-blocking method to receive the next incoming connection if present.
    /// See tokio::sync::mpsc::Receiver::try_recv()
    pub fn try_recv(&mut self) -> Result<(Connection, ConnectionIncoming), TryRecvError> {
        self.0.try_recv()
    }
}

/// Endpoint instance which can be used to communicate with peers.
#[derive(Clone)]
pub struct Endpoint {
    local_addr: SocketAddr,
    public_addr: Option<SocketAddr>,
    quinn_endpoint: QuinnEndpoint,

    termination_tx: Sender<()>,
}

impl std::fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Endpoint")
            .field("local_addr", &self.local_addr)
            .field("quinn_endpoint", &"<endpoint omitted>")
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
    pub async fn new_peer(
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

        let (termination_tx, termination_rx) = broadcast::channel(1);

        let (mut quinn_endpoint, quinn_incoming) =
            QuinnEndpoint::server(config.server.clone(), local_addr.into())?;

        // set client config used for any outgoing connections
        quinn_endpoint.set_default_client_config(config.client);

        // Get actual socket address.
        let local_addr = quinn_endpoint.local_addr()?;

        let mut endpoint = Self {
            local_addr,
            public_addr: None, // we'll set this below
            quinn_endpoint,
            termination_tx,
        };

        let contact = endpoint.connect_to_any(contacts).await;

        let public_addr = endpoint
            .resolve_public_addr(
                config.external_ip,
                config.external_port,
                contact.as_ref().map(|c| &c.0),
            )
            .await?;

        endpoint.public_addr = Some(public_addr);

        drop(termination_rx);

        let (connection_tx, connection_rx) = mpsc::channel(STANDARD_CHANNEL_SIZE);

        listen_for_incoming_connections(
            quinn_incoming,
            connection_tx,
            endpoint.quinn_endpoint.clone(),
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

        let (termination_tx, _termination_rx) = broadcast::channel(1);

        let local_addr = local_addr.into();

        let mut quinn_endpoint = QuinnEndpoint::client(local_addr)?;

        // retrieve the actual used socket addr
        let local_quinn_socket_addr = quinn_endpoint.local_addr()?;

        quinn_endpoint.set_default_client_config(config.client);

        let endpoint = Self {
            local_addr: local_quinn_socket_addr,
            public_addr: None, // we're a client
            quinn_endpoint,
            termination_tx,
        };

        Ok(endpoint)
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
    /// Atttempts to connect to a peer at the given address.
    ///
    /// Returns a [`Connection`], which is a connection instance.
    ///
    /// **Note:** this method is intended for use when it's necessary to connect to a specific peer.
    /// See [`connect_to_any`](Self::connect_to_any) if you just need a connection with any of a set
    /// of peers.
    pub async fn connect_to(
        &self,
        node_addr: &SocketAddr,
    ) -> Result<(Connection, ConnectionIncoming), ConnectionError> {
        self.new_connection(node_addr).await
    }

    /// Connect to any of the given peers.
    ///
    /// Often in peer-to-peer networks, it's sufficient to communicate to any node on the network,
    /// rather than having to connect to specific nodes. This method will start connecting to every
    /// peer in `peer_addrs`, and return the address of the first successfully established
    /// connection (the rest are cancelled and discarded).
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
            .map(|addr| Box::pin(self.new_connection(addr)));

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

        let (connection, _) = self.new_connection(peer_addr).await?;
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
        trace!("Closing endpoint");
        let _ = self.termination_tx.send(());
        self.quinn_endpoint.close(0_u32.into(), b"Endpoint closed")
    }

    /// Attempt a connection to a node_addr.
    ///
    /// It will always try to open a new connection.
    async fn new_connection(
        &self,
        node_addr: &SocketAddr,
    ) -> Result<(Connection, ConnectionIncoming), ConnectionError> {
        trace!("Attempting to connect to {:?}", node_addr);
        let connecting = match self.quinn_endpoint.connect(*node_addr, SERVER_NAME) {
            Ok(conn) => Ok(conn),
            Err(error) => {
                warn!("Connection attempt failed due to {:?}", error);
                Err(ConnectionError::from(error))
            }
        }?;

        let new_conn = match connecting.await {
            Ok(new_conn) => {
                trace!("Successfully connected to peer: {}", node_addr);

                let (connection, connection_incoming) =
                    Connection::new(self.quinn_endpoint.clone(), new_conn);

                Ok((connection, connection_incoming))
            }
            Err(error) => Err(ConnectionError::from(error)),
        }?;

        Ok(new_conn)
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
            msg => Err(RpcError::EchoResponseMissing {
                peer: contact.remote_address(),
                response: msg.map(|m| m.to_string()),
            }),
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
            msg => Err(RpcError::EndpointVerificationRespMissing {
                peer: contact.remote_address(),
                response: msg.map(|m| m.to_string()),
            }),
        }
    }
}

pub(super) fn listen_for_incoming_connections(
    mut quinn_incoming: quinn::Incoming,
    connection_tx: mpsc::Sender<(Connection, ConnectionIncoming)>,
    quinn_endpoint: quinn::Endpoint,
) {
    let _ = tokio::spawn(async move {
        loop {
            match quinn_incoming.next().await {
                Some(quinn_conn) => match quinn_conn.await {
                    Ok(connection) => {
                        let (connection, connection_incoming) =
                            Connection::new(quinn_endpoint.clone(), connection);

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
        let (endpoint, _, _) = Endpoint::new_peer(
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
        let (endpoint, _, _) = Endpoint::new_peer(
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
        let (endpoint, _, _) = Endpoint::new_peer(
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
        let (endpoint, _, _) = Endpoint::new_peer(
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
