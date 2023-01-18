// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::connection::ConnectionIncoming;
use crate::EndpointBuilder;

use super::wire_msg::WireMsg;
use super::{
    config::{Config, InternalConfig, SERVER_NAME},
    connection::Connection,
    error::{ClientEndpointError, ConnectionError, EndpointError, RpcError},
};
use std::net::SocketAddr;
use tokio::sync::mpsc::{self, error::TryRecvError, Receiver};
use tokio::time::{timeout, Duration};
use tracing::{error, info, trace, warn};

// Number of seconds before timing out the echo service query.
const ECHO_SERVICE_QUERY_TIMEOUT: Duration = Duration::from_secs(30);

/// Standard size of our channel bounds
const STANDARD_CHANNEL_SIZE: usize = 10_000;

/// Channel on which incoming connections are notified on
#[derive(Debug)]
pub struct IncomingConnections(pub(crate) Receiver<(Connection, ConnectionIncoming)>);

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
    pub(crate) inner: quinn::Endpoint,
    pub(crate) local_addr: SocketAddr,
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
    pub async fn new_peer(
        local_addr: impl Into<SocketAddr>,
        config: Config,
    ) -> Result<(Self, IncomingConnections), EndpointError> {
        let config = InternalConfig::try_from_config(config)?;

        let mut quinn_endpoint = quinn::Endpoint::server(config.server.clone(), local_addr.into())?;

        // set client config used for any outgoing connections
        quinn_endpoint.set_default_client_config(config.client);

        // Get actual socket address.
        let local_addr = quinn_endpoint.local_addr()?;

        let endpoint = Self {
            local_addr,
            inner: quinn_endpoint,
        };

        let (connection_tx, connection_rx) = mpsc::channel(STANDARD_CHANNEL_SIZE);

        listen_for_incoming_connections(endpoint.inner.clone(), connection_tx);

        Ok((endpoint, IncomingConnections(connection_rx)))
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

        let mut quinn_endpoint = quinn::Endpoint::client(local_addr.into())?;

        quinn_endpoint.set_default_client_config(config.client);

        // retrieve the actual used socket addr
        let local_addr = quinn_endpoint.local_addr()?;

        let endpoint = Self {
            local_addr,
            inner: quinn_endpoint,
        };

        Ok(endpoint)
    }

    /// Endpoint local address
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
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

        match timeout(ECHO_SERVICE_QUERY_TIMEOUT, recv_stream.read_wire_msg()).await?? {
            WireMsg::EndpointEchoResp(_) => Ok(()),
            other => {
                info!(
                    "Unexpected message type when verifying reachability: {}",
                    &other
                );
                Ok(())
            }
        }
    }

    /// Close all the connections of this endpoint immediately and stop accepting new connections.
    pub fn close(&self) {
        trace!("Closing endpoint");
        self.inner.close(0_u32.into(), b"Endpoint closed")
    }

    /// Attempt a connection to a node_addr.
    ///
    /// It will always try to open a new connection.
    async fn new_connection(
        &self,
        node_addr: &SocketAddr,
    ) -> Result<(Connection, ConnectionIncoming), ConnectionError> {
        trace!("Attempting to connect to {:?}", node_addr);
        let connecting = match self.inner.connect(*node_addr, SERVER_NAME) {
            Ok(conn) => Ok(conn),
            Err(error) => {
                warn!(
                    "Connection attempt to {node_addr:?} failed due to {:?}",
                    error
                );
                Err(ConnectionError::from(error))
            }
        }?;

        let new_conn = match connecting.await {
            Ok(new_conn) => {
                let connection = Connection::new(new_conn, self.inner.clone());
                trace!(
                    "Successfully connected to peer {node_addr}, conn_id={}",
                    connection.0.id()
                );
                Ok(connection)
            }
            Err(error) => Err(ConnectionError::from(error)),
        }?;

        Ok(new_conn)
    }

    /// Builder to create an `Endpoint`.
    pub fn builder() -> EndpointBuilder {
        EndpointBuilder::default()
    }
}

pub(super) fn listen_for_incoming_connections(
    quinn_endpoint: quinn::Endpoint,
    connection_tx: mpsc::Sender<(Connection, ConnectionIncoming)>,
) {
    let _ = tokio::spawn(async move {
        while let Some(quinn_conn) = quinn_endpoint.accept().await {
            match quinn_conn.await {
                Ok(connection) => {
                    let connection = Connection::new(connection, quinn_endpoint.clone());
                    let conn_id = connection.0.id();
                    trace!("Incoming new connection conn_id={conn_id}");
                    if connection_tx.send(connection).await.is_err() {
                        warn!("Dropping incoming connection conn_id={conn_id}, because receiver was dropped");
                    }
                }
                Err(err) => {
                    warn!("An incoming connection failed because of: {:?}", err);
                }
            }
        }

        trace!(
            "quinn::Endpoint::accept() returned None. There will be no more incoming connections"
        );
    });
}
