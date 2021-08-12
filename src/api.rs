// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use super::{
    config::{Config, InternalConfig, SerialisableCertificate},
    connection_pool::ConnId,
    connections::DisconnectionEvents,
    endpoint::{Endpoint, IncomingConnections, IncomingMessages},
    error::{Error, Result},
    peer_config::{self, DEFAULT_IDLE_TIMEOUT_MSEC, DEFAULT_KEEP_ALIVE_INTERVAL_MSEC},
};
use std::marker::PhantomData;
use std::net::{SocketAddr, UdpSocket};
use tracing::{debug, error, trace};

/// Default duration of a UPnP lease, in seconds.
pub(crate) const DEFAULT_UPNP_LEASE_DURATION_SEC: u32 = 120;

const MAIDSAFE_DOMAIN: &str = "maidsafe.net";

/// Main QuicP2p instance to communicate with QuicP2p using an async API
#[derive(Debug, Clone)]
pub struct QuicP2p<I: ConnId> {
    endpoint_cfg: quinn::ServerConfig,
    client_cfg: quinn::ClientConfig,
    qp2p_config: InternalConfig,
    phantom: PhantomData<I>,
}

impl<I: ConnId> QuicP2p<I> {
    /// Construct `QuicP2p` with supplied configuration.
    ///
    /// If `config` is `None`, the default value will be used.
    ///
    /// # Example
    ///
    /// ```
    /// use qp2p::{QuicP2p, Config, ConnId};
    /// use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    ///
    /// # #[derive(Default, Ord, PartialEq, PartialOrd, Eq, Clone, Copy)]
    /// # struct XId(pub [u8; 32]);
    /// #
    /// # impl ConnId for XId {
    /// #     fn generate(_socket_addr: &SocketAddr) -> Result<Self, Box<dyn std::error::Error>> {
    /// #         Ok(XId(rand::random()))
    /// #     }
    /// # }
    ///
    /// let quic_p2p = QuicP2p::<XId>::with_config(Config::default())
    ///     .expect("Error initializing QuicP2p");
    /// ```
    pub fn with_config(cfg: Config) -> Result<Self> {
        debug!("Config passed in to qp2p: {:?}", cfg);

        let idle_timeout_msec = cfg.idle_timeout_msec.unwrap_or(DEFAULT_IDLE_TIMEOUT_MSEC);

        let keep_alive_interval_msec = cfg
            .keep_alive_interval_msec
            .unwrap_or(DEFAULT_KEEP_ALIVE_INTERVAL_MSEC);

        let (key, cert) = {
            let our_complete_cert =
                SerialisableCertificate::new(vec![MAIDSAFE_DOMAIN.to_string()])?;
            our_complete_cert.obtain_priv_key_and_cert()?
        };

        let endpoint_cfg =
            peer_config::new_our_cfg(idle_timeout_msec, keep_alive_interval_msec, cert, key)?;

        let client_cfg = peer_config::new_client_cfg(idle_timeout_msec, keep_alive_interval_msec)?;

        let upnp_lease_duration = cfg
            .upnp_lease_duration
            .unwrap_or(DEFAULT_UPNP_LEASE_DURATION_SEC);

        let qp2p_config = InternalConfig {
            forward_port: cfg.forward_port,
            external_port: cfg.external_port,
            external_ip: cfg.external_ip,
            upnp_lease_duration,
            retry_duration_msec: cfg.retry_duration_msec,
        };

        Ok(Self {
            endpoint_cfg,
            client_cfg,
            qp2p_config,
            phantom: PhantomData::default(),
        })
    }

    /// Bootstrap to the network.
    ///
    /// Bootstrapping will attempt to connect to all the given peers. The first successful
    /// connection to will be returned, and the others will be dropped.
    ///
    /// # Example
    ///
    /// ```
    /// use qp2p::{QuicP2p, Config, Error, ConnId};
    /// use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    ///
    /// # #[derive(Default, Ord, PartialEq, PartialOrd, Eq, Clone, Copy)]
    /// # struct XId(pub [u8; 32]);
    /// #
    /// # impl ConnId for XId {
    /// #     fn generate(_socket_addr: &SocketAddr) -> Result<Self, Box<dyn std::error::Error>> {
    /// #         Ok(XId(rand::random()))
    /// #     }
    /// # }
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Error> {
    ///     let local_addr = (IpAddr::V4(Ipv4Addr::LOCALHOST), 3000).into();
    ///     let quic_p2p = QuicP2p::<XId>::with_config(Config::default())?;
    ///     let (mut endpoint, _, _, _) = quic_p2p.new_endpoint(local_addr).await?;
    ///     let peer_addr = endpoint.socket_addr();
    ///
    ///     let local_addr = (IpAddr::V4(Ipv4Addr::LOCALHOST), 3001).into();
    ///     let endpoint = quic_p2p.bootstrap(local_addr, vec![peer_addr]).await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn bootstrap(
        &self,
        local_addr: SocketAddr,
        bootstrap_nodes: Vec<SocketAddr>,
    ) -> Result<(
        Endpoint<I>,
        IncomingConnections,
        IncomingMessages,
        DisconnectionEvents,
        SocketAddr,
    )> {
        let (endpoint, incoming_connections, incoming_message, disconnections) =
            self.new_endpoint_with(local_addr, bootstrap_nodes).await?;

        let bootstrapped_peer = endpoint.connect_to_any(endpoint.bootstrap_nodes()).await?;

        Ok((
            endpoint,
            incoming_connections,
            incoming_message,
            disconnections,
            bootstrapped_peer,
        ))
    }

    /// Create a new [`Endpoint`] which can be used to interact with a network.
    ///
    /// `Endpoint`s can send messages to reachable peers, as well as listen to messages incoming
    /// from other peers.
    ///
    /// # Example
    ///
    /// ```
    /// use qp2p::{QuicP2p, Config, Error, ConnId};
    /// use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    ///
    /// # #[derive(Default, Ord, PartialEq, PartialOrd, Eq, Clone, Copy)]
    /// # struct XId(pub [u8; 32]);
    /// #
    /// # impl ConnId for XId {
    /// #     fn generate(_socket_addr: &SocketAddr) -> Result<Self, Box<dyn std::error::Error>> {
    /// #         Ok(XId(rand::random()))
    /// #     }
    /// # }
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Error> {
    ///     let local_addr = (IpAddr::V4(Ipv4Addr::LOCALHOST), 0).into();
    ///     let quic_p2p = QuicP2p::<XId>::with_config(Config::default())?;
    ///     let (endpoint, incoming_connections, incoming_messages, disconnections) = quic_p2p.new_endpoint(local_addr).await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn new_endpoint(
        &self,
        local_addr: SocketAddr,
    ) -> Result<(
        Endpoint<I>,
        IncomingConnections,
        IncomingMessages,
        DisconnectionEvents,
    )> {
        self.new_endpoint_with(local_addr, Default::default()).await
    }

    async fn new_endpoint_with(
        &self,
        local_addr: SocketAddr,
        bootstrap_nodes: Vec<SocketAddr>,
    ) -> Result<(
        Endpoint<I>,
        IncomingConnections,
        IncomingMessages,
        DisconnectionEvents,
    )> {
        trace!("Creating a new endpoint");

        let (quinn_endpoint, quinn_incoming) = bind(self.endpoint_cfg.clone(), local_addr)?;

        trace!(
            "Bound endpoint to local address: {}",
            quinn_endpoint.local_addr()?
        );

        let endpoint = Endpoint::new(
            quinn_endpoint,
            quinn_incoming,
            self.client_cfg.clone(),
            bootstrap_nodes,
            self.qp2p_config.clone(),
        )
        .await?;

        Ok(endpoint)
    }
}

// Bind a new socket with a local address
pub(crate) fn bind(
    endpoint_cfg: quinn::ServerConfig,
    local_addr: SocketAddr,
) -> Result<(quinn::Endpoint, quinn::Incoming)> {
    let mut endpoint_builder = quinn::Endpoint::builder();
    let _ = endpoint_builder.listen(endpoint_cfg);

    match UdpSocket::bind(&local_addr) {
        Ok(udp) => endpoint_builder.with_socket(udp).map_err(Error::Endpoint),
        Err(err) => {
            error!("{}", err);
            Err(Error::CannotAssignPort(local_addr.port()))
        }
    }
}
