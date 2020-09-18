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
use super::{
    connections::{Connection, IncomingConnections},
    error::{Error, Result},
};
use futures::lock::Mutex;
use log::trace;
#[cfg(feature = "upnp")]
use log::{debug, info};
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
    upnp_lease_duration: u32,
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
        upnp_lease_duration: u32,
    ) -> Result<Self> {
        let local_addr = quic_endpoint.local_addr()?;
        if local_addr.ip().is_unspecified() {
            Err(Error::Configuration(
                "No IP specified in the config and IGD detection is disabled or not available."
                    .to_string(),
            ))
        } else {
            Ok(Self {
                local_addr,
                quic_endpoint,
                quic_incoming: Arc::new(Mutex::new(quic_incoming)),
                client_cfg,
                upnp_lease_duration,
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
    pub async fn our_addr(&self) -> Result<SocketAddr> {
        // Make use of UPnP to detect our public addr
        let is_loopback = self.local_addr.ip().is_loopback();

        // In parallel, try to contact an echo service
        let echo_service_res = match self.query_ip_echo_service() {
            Ok(addr) => Ok(addr),
            Err(Error::NoEndpointEchoServerFound) => Ok(self.local_addr),
            Err(err) => {
                info!("Could not contact echo service: {} - {:?}", err, err);
                Err(err)
            }
        };

        let mut addr = None;
        // Skip port forwarding if we are running locally
        if !is_loopback {
            // Attempt to use IGD for port forwarding
            match forward_port(self.local_addr, self.upnp_lease_duration).await {
                Ok(public_sa) => {
                    debug!("IGD success: {:?}", SocketAddr::V4(public_sa));
                    let mut local_addr = self.local_addr;
                    local_addr.set_port(public_sa.port());
                    addr = Some(local_addr)
                }
                Err(e) => {
                    info!("IGD request failed: {} - {:?}", e, e);
                    return Err(Error::IgdNotSupported);
                }
            }
        }

        echo_service_res.map(|echo_srvc_addr| {
            if let Some(our_address) = addr {
                our_address
            } else {
                echo_srvc_addr
            }
        })
    }

    /// Endpoint local address to give others for them to connect to us.
    #[cfg(not(feature = "upnp"))]
    pub fn our_addr(&self) -> Result<SocketAddr> {
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
    fn query_ip_echo_service(&self) -> Result<SocketAddr> {
        Ok(self.local_addr)
        /*    // Bail out early if we don't have any contacts.
        if self.cfg.hard_coded_contacts.is_empty() {
            return Err(QuicP2pError::NoEndpointEchoServerFound);
        }

        // Create a separate event stream for the IP echo request.
        let (echo_resp_tx, echo_resp_rx) = mpsc::channel();

        let idle_timeout_msec = Duration::from_millis(
            self.cfg
                .idle_timeout_msec
                .unwrap_or(DEFAULT_IDLE_TIMEOUT_MSEC),
        );

        self.el.post(move || {
            ctx_mut(|ctx| {
                ctx.our_ext_addr_tx = Some(echo_resp_tx);
            });
        });

        debug!(
            "Querying IP echo service to find our public IP address (contact list: {:?})",
            self.cfg.hard_coded_contacts
        );

        loop {
            if self.cfg.hard_coded_contacts.is_empty() {
                return Err(QuicP2pError::NoEndpointEchoServerFound);
            }

            let (notify_tx, notify_rx) = utils::new_unbounded_channels();

            self.el.post(move || {
                let _ = bootstrap::echo_request(notify_tx);
            });

            match notify_rx.recv_timeout(idle_timeout_msec) {
                Ok(Event::BootstrapFailure { .. }) => {
                    debug!("BootstrapFailure");
                    break Err(QuicP2pError::NoEndpointEchoServerFound);
                }
                Ok(Event::BootstrappedTo { node }) => {
                    debug!("BootstrappedTo {{ node: {:?} }}", node);
                    match echo_resp_rx.recv_timeout(idle_timeout_msec) {
                        Ok(res) => {
                            debug!("Found our address: {:?}", res);
                            break Ok(res);
                        }
                        Err(_e) => {
                            // This node hasn't replied in a timely manner, so remove it from our bootstrap list and try again.
                            let _ = self.cfg.hard_coded_contacts.remove(&node);
                            info!(
                "Node {} is unresponsive, removing it from bootstrap contacts; {} contacts left",
                                node,
                                self.cfg.hard_coded_contacts.len()
                            );
                        }
                    }
                }
                Ok(ev) => {
                    debug!("Unexpected event: {:?}", ev);
                    break Err(QuicP2pError::NoEndpointEchoServerFound);
                }
                Err(err) => {
                    debug!("Unexpected error: {:?}", err);
                    break Err(QuicP2pError::NoEndpointEchoServerFound);
                }
            }
        }*/
    }
}
