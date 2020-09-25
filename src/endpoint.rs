// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

#[cfg(feature = "upnp")]
use super::igd::forward_port;
#[cfg(feature = "upnp")]
use log::{debug, info};
#[cfg(feature = "upnp")]
use super::error::Error;
use super::{
    connections::{Connection, IncomingConnections},
    error::Result,
};
use super::wire_msg::WireMsg;
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
    #[cfg(feature = "upnp")]
    upnp_lease_duration: u32,
    #[cfg(feature = "upnp")]
    bootstrap_nodes: Vec<SocketAddr>,
}

impl std::fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Endpoint")
            .field("local_addr", &self.local_addr)
            .field("quic_endpoint", &"<endpoint omitted>".to_string())
            .field("quic_incoming", &self.quic_incoming)
            .field("client_cfg", &self.client_cfg)
            .finish()
    }
}

impl Endpoint {
    pub(crate) fn new(
        quic_endpoint: quinn::Endpoint,
        quic_incoming: quinn::Incoming,
        client_cfg: quinn::ClientConfig,
        #[cfg(feature = "upnp")]
        upnp_lease_duration: u32,
        #[cfg(feature = "upnp")]
        bootstrap_nodes: Vec<SocketAddr>,
    ) -> Result<Self> {
        let local_addr = quic_endpoint.local_addr()?;
        dbg!(local_addr);
        Ok(Self {
            local_addr,
            quic_endpoint,
            quic_incoming: Arc::new(Mutex::new(quic_incoming)),
            client_cfg,
            #[cfg(feature = "upnp")]
            upnp_lease_duration,
            #[cfg(feature = "upnp")]
            bootstrap_nodes,
        })
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
    pub async fn our_endpoint(&mut self) -> Result<SocketAddr> {

        // Skip port forwarding
        if self.local_addr.ip().is_loopback() || !self.local_addr.ip().is_unspecified() {
            return Ok(self.local_addr);
        }

        let mut addr = None;

        // Attempt to use IGD for port forwarding
        match forward_port(self.local_addr, self.upnp_lease_duration).await {
            Ok(public_sa) => {
                debug!("IGD success: {:?}", SocketAddr::V4(public_sa));
                addr = Some(SocketAddr::V4(public_sa));
            }
            Err(e) => {
                info!("IGD request failed: {} - {:?}", e, e);
                // return Err(Error::IgdNotSupported);
            }
        }

        // Try to contact an echo service
        match self.query_ip_echo_service().await {
            Ok(echo_res) => {
                match addr {
                    None => {
                        addr = Some(echo_res);
                    },
                    Some(address) => {
                        info!("Got response from echo service: {:?}, but IGD has already provided our external address: {:?}", echo_res, address);
                    }
                }
            },
            Err(err) => {
                info!("Could not contact echo service: {} - {:?}", err, err);
            }
        };
        if let Some(socket_addr) = addr {
            Ok(socket_addr)
        } else {
            Err(Error::Unexpected("No response from echo service".to_string()))   
        }
    }

    /// Endpoint local address to give others for them to connect to us.
    #[cfg(not(feature = "upnp"))]
    pub async fn our_endpoint(&mut self) -> Result<SocketAddr> {
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

    // Private helper
    #[cfg(feature = "upnp")]
    async fn query_ip_echo_service(&self) -> Result<SocketAddr> {
        // Bail out early if we don't have any contacts.
        if self.bootstrap_nodes.is_empty() {
            return Err(Error::NoEndpointEchoServerFound);
        }

        let mut tasks = Vec::default();
        for node in self.bootstrap_nodes.iter().cloned() {
            let connection = self.connect_to(&node).await?; // TODO: move into loop
            let task_handle = tokio::spawn(async move {
                let (mut send_stream, mut recv_stream) = connection.open_bi_stream().await?;
                send_stream.send(WireMsg::EndpointEchoReq).await?;
                match WireMsg::read_from_stream(&mut recv_stream.quinn_recv_stream).await {
                    Ok(WireMsg::EndpointEchoResp(socket_addr)) => Ok(socket_addr),
                    Ok(_) => Err(Error::Unexpected("Unexpected message".to_string())),
                    Err(err) => Err(err),
                }
            });
            tasks.push(task_handle);
        }

        self.local_addr()

    }
}
