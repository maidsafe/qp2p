// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::EndpointBuilder;
use crate::{connection::ConnectionIncoming, endpoint_builder::SERVER_NAME};

use super::{connection::Connection, error::ConnectionError};
use std::net::SocketAddr;
use tokio::sync::mpsc::{self, error::TryRecvError, Receiver};
use tracing::{error, trace, warn};

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

    /// Clean up endpoint resources.
    /// Will do so cleanly and wait for all incoming connections to close.
    pub async fn close(&self) {
        trace!("Closing endpoint");
        self.inner.wait_idle().await; // wait for all connections to close
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
                let connection = Connection::new(new_conn);
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
    let _handle = tokio::spawn(async move {
        while let Some(quinn_conn) = quinn_endpoint.accept().await {
            let conn_sender = connection_tx.clone();
            // move incoming conn waiting off thread so as not to block us
            let _handle = tokio::spawn(async move {
                match quinn_conn.await {
                    Ok(connection) => {
                        let connection = Connection::new(connection);
                        let conn_id = connection.0.id();
                        trace!("Incoming new connection conn_id={conn_id}");
                        if conn_sender.send(connection).await.is_err() {
                            warn!("Dropping incoming connection conn_id={conn_id}, because receiver was dropped");
                        }
                    }
                    Err(err) => {
                        warn!("An incoming connection failed because of: {:?}", err);
                    }
                }
            });
        }

        trace!(
            "quinn::Endpoint::accept() returned None. There will be no more incoming connections"
        );
    });
}
