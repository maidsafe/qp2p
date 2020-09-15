// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use super::{
    connections::{Connection, IncomingConnections},
    error::{Error, Result},
};
use futures::lock::Mutex;
use log::trace;
use std::{net::SocketAddr, sync::Arc};

/// Host name of the Quic communication certificate used by peers
// FIXME: make it configurable
const CERT_SERVER_NAME: &str = "MaidSAFE.net";

/// Endpoint instance which can be used to create connections to peers,
/// and listen to incoming messages from other peers.
pub struct Endpoint {
    local_addr: SocketAddr,
    quic_endpoint: quinn::Endpoint,
    quic_incoming: Arc<Mutex<quinn::Incoming>>,
    client_cfg: quinn::ClientConfig,
}

impl Endpoint {
    pub(crate) fn new(
        quic_endpoint: quinn::Endpoint,
        quic_incoming: quinn::Incoming,
        client_cfg: quinn::ClientConfig,
    ) -> Result<Self> {
        let local_addr = quic_endpoint.local_addr()?;
        if local_addr.ip().is_unspecified() {
            Err(Error::Configuration(
                "No IP not specified in the config and IGD detection is disabled or not available."
                    .to_string(),
            ))
        } else {
            Ok(Self {
                local_addr,
                quic_endpoint,
                quic_incoming: Arc::new(Mutex::new(quic_incoming)),
                client_cfg,
            })
        }
    }

    /// Endpoint local address
    pub fn local_addr(&self) -> Result<SocketAddr> {
        Ok(self.local_addr)
    }

    /// Get our connection adddress to give to others for them to connect to us.
    ///
    /// Attempts to use UPnP to automatically find the public endpoint and forward a port.
    /// Will use hard coded contacts to ask for our endpoint. If no contact is given then we'll
    /// simply build our connection info by querying the underlying bound socket for our address.
    /// Note that if such an obtained address is of unspecified category we will ignore that as
    /// such an address cannot be reached and hence not useful.
    #[cfg(feature = "upnp")]
    pub fn our_endpoint(&self) -> Result<SocketAddr> {
        // TODO: make use of UPnP
        self.local_addr()
    }

    /// Endpoint local address to give others for them to connect to us.
    #[cfg(not(feature = "upnp"))]
    pub fn our_endpoint(&self) -> Result<SocketAddr> {
        self.local_addr()
    }

    /// Connect to another peer
    pub async fn connect_to(&self, node_addr: &SocketAddr) -> Result<Connection> {
        let quinn_connecting = self.quic_endpoint.connect_with(
            self.client_cfg.clone(),
            &node_addr,
            CERT_SERVER_NAME,
        )?;

        let quinn::NewConnection {
            connection: quinn_conn,
            ..
        } = quinn_connecting.await?;

        trace!("Successfully connected to peer: {}", node_addr);

        Connection::new(quinn_conn).await
    }

    /// Obtain stream of incoming QUIC connections
    pub fn listen(&self) -> Result<IncomingConnections> {
        trace!(
            "Incoming connections will be received at {}",
            self.quic_endpoint.local_addr()?
        );
        IncomingConnections::new(Arc::clone(&self.quic_incoming))
    }
}
