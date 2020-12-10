// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use super::{
    bootstrap_cache::BootstrapCache,
    config::{Config, SerialisableCertificate},
    connections::{Connection, IncomingMessages, RecvStream, SendStream},
    endpoint::Endpoint,
    error::{Error, Result},
    peer_config::{self, DEFAULT_IDLE_TIMEOUT_MSEC, DEFAULT_KEEP_ALIVE_INTERVAL_MSEC},
};
use bytes::Bytes;
use futures::future;
use log::{debug, error, info, trace};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::path::PathBuf;

/// In the absence of a port supplied by the user via the config we will first try using this
/// before using a random port.
pub const DEFAULT_PORT_TO_TRY: u16 = 12000;

/// Default duration of a UPnP lease, in seconds.
pub const DEFAULT_UPNP_LEASE_DURATION_SEC: u32 = 120;

const MAIDSAFE_DOMAIN: &str = "maidsafe.net";

/// Message received from a peer
#[derive(Debug)]
pub enum Message {
    /// A message sent by peer on a uni-directional stream
    UniStream {
        /// Message's bytes
        bytes: Bytes,
        /// Address the message was sent from
        src: SocketAddr,
        /// Stream to read more messages
        recv: RecvStream,
    },
    /// A message sent by peer on a bi-directional stream
    BiStream {
        /// Message's bytes
        bytes: Bytes,
        /// Address the message was sent from
        src: SocketAddr,
        /// Stream to send a message back to the initiator
        send: SendStream,
        /// Stream to read more messages
        recv: RecvStream,
    },
}

impl Message {
    /// Returns the data from the message
    pub fn get_message_data(&self) -> Bytes {
        match self {
            Self::UniStream { bytes, .. } | Self::BiStream { bytes, .. } => bytes.clone(),
        }
    }
}

/// Main QuicP2p instance to communicate with QuicP2p using an async API
#[derive(Debug, Clone)]
pub struct QuicP2p {
    local_addr: SocketAddr,
    allow_random_port: bool,
    bootstrap_cache: BootstrapCache,
    endpoint_cfg: quinn::ServerConfig,
    client_cfg: quinn::ClientConfig,
    qp2p_config: Config,
}

impl QuicP2p {
    /// Construct `QuicP2p` with the default config and bootstrap cache enabled
    ///
    /// # Example
    ///
    /// ```
    /// use qp2p::QuicP2p;
    ///
    /// let quic_p2p = QuicP2p::new().expect("Error initializing QuicP2p");
    /// ```
    pub fn new() -> Result<Self> {
        Self::with_config(None, Default::default(), true)
    }

    /// Construct `QuicP2p` with supplied parameters, ready to be used.
    /// If config is not specified it'll call `Config::read_or_construct_default()`
    ///
    /// `bootstrap_nodes`: takes bootstrap nodes from the user.
    ///
    /// In addition to bootstrap nodes provided, optionally use the nodes found
    /// in the bootstrap cache file (if such a file exists) or disable this feature.
    ///
    /// # Example
    ///
    /// ```
    /// use qp2p::{QuicP2p, Config};
    /// use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    ///
    /// let mut config = Config::default();
    /// config.ip = Some(IpAddr::V4(Ipv4Addr::LOCALHOST));
    /// config.port = Some(3000);
    /// let hcc = &["127.0.0.1:8080".parse().unwrap()];
    /// let quic_p2p = QuicP2p::with_config(Some(config), hcc, true).expect("Error initializing QuicP2p");
    /// ```
    pub fn with_config(
        cfg: Option<Config>,
        bootstrap_nodes: &[SocketAddr],
        use_bootstrap_cache: bool,
    ) -> Result<Self> {
        let cfg = unwrap_config_or_default(cfg)?;
        debug!("Config passed in to qp2p: {:?}", cfg);

        let (port, allow_random_port) = cfg
            .port
            .map(|p| (p, false))
            .unwrap_or((DEFAULT_PORT_TO_TRY, true));

        let ip = cfg.ip.unwrap_or_else(|| {
            let mut our_ip = IpAddr::V4(Ipv4Addr::UNSPECIFIED);

            // check hard coded contacts for being local (aka loopback)
            if let Some(contact) = cfg.hard_coded_contacts.iter().next() {
                let ip = contact.ip();

                if ip.is_loopback() {
                    trace!(
                        "IP from hardcoded contact is loopback, setting our IP to: {:?}",
                        ip
                    );
                    our_ip = ip;
                }
            }

            our_ip
        });

        let idle_timeout_msec = cfg.idle_timeout_msec.unwrap_or(DEFAULT_IDLE_TIMEOUT_MSEC);

        let keep_alive_interval_msec = cfg
            .keep_alive_interval_msec
            .unwrap_or(DEFAULT_KEEP_ALIVE_INTERVAL_MSEC);

        let (key, cert) = {
            let our_complete_cert =
                SerialisableCertificate::new(vec![MAIDSAFE_DOMAIN.to_string()])?;
            our_complete_cert.obtain_priv_key_and_cert()?
        };

        let custom_dirs = cfg.bootstrap_cache_dir.clone().map(PathBuf::from);

        let mut qp2p_config = cfg.clone();

        if cfg.clean {
            BootstrapCache::clear_from_disk(custom_dirs.as_ref())?;
        }

        let mut bootstrap_cache =
            BootstrapCache::new(cfg.hard_coded_contacts, custom_dirs.as_ref(), cfg.fresh)?;
        if use_bootstrap_cache {
            bootstrap_cache.peers_mut().extend(bootstrap_nodes);
        } else {
            let bootstrap_cache = bootstrap_cache.peers_mut();
            bootstrap_cache.clear();
            for addr in bootstrap_nodes {
                bootstrap_cache.push_back(*addr);
            }
        }

        let endpoint_cfg =
            peer_config::new_our_cfg(idle_timeout_msec, keep_alive_interval_msec, cert, key)?;

        let client_cfg = peer_config::new_client_cfg(idle_timeout_msec, keep_alive_interval_msec)?;

        let upnp_lease_duration = cfg
            .upnp_lease_duration
            .unwrap_or(DEFAULT_UPNP_LEASE_DURATION_SEC);

        qp2p_config.ip = Some(ip);
        qp2p_config.port = Some(port);
        qp2p_config.keep_alive_interval_msec = Some(keep_alive_interval_msec);
        qp2p_config.idle_timeout_msec = Some(idle_timeout_msec);
        qp2p_config.upnp_lease_duration = Some(upnp_lease_duration);

        Ok(Self {
            local_addr: SocketAddr::new(ip, port),
            allow_random_port,
            bootstrap_cache,
            endpoint_cfg,
            client_cfg,
            qp2p_config,
        })
    }

    /// Bootstrap to the network.
    ///
    /// Bootstrap concept is different from "connect" in several ways: `bootstrap()` will try to
    /// connect to all peers which are specified in the config (`hard_coded_contacts`) or were
    /// previously cached.
    /// Once a connection with a peer succeeds, a `Connection` for such peer will be returned
    /// and all other connections will be dropped.
    ///
    /// # Example
    ///
    /// ```
    /// use qp2p::{QuicP2p, Config, Error};
    /// use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Error> {
    ///
    ///     let mut config = Config::default();
    ///     config.ip = Some(IpAddr::V4(Ipv4Addr::LOCALHOST));
    ///     config.port = Some(3000);
    ///     let mut quic_p2p = QuicP2p::with_config(Some(config.clone()), Default::default(), true)?;
    ///     let endpoint = quic_p2p.new_endpoint()?;
    ///     let peer_addr = endpoint.socket_addr().await?;
    ///
    ///     config.port = Some(3001);
    ///     let mut quic_p2p = QuicP2p::with_config(Some(config), &[peer_addr], true)?;
    ///     let (endpoint, connection, incoming_messages) = quic_p2p.bootstrap().await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn bootstrap(&self) -> Result<(Endpoint, Connection, IncomingMessages)> {
        let endpoint = self.new_endpoint()?;

        trace!("Bootstrapping with nodes {:?}", endpoint.bootstrap_nodes());
        if endpoint.bootstrap_nodes().is_empty() {
            return Err(Error::Unexpected(
                "No nodes for bootstrapping found".to_string(),
            ));
        }

        // Attempt to connect to all nodes and return the first one to succeed
        let tasks = endpoint
            .bootstrap_nodes()
            .iter()
            .map(|addr| Box::pin(endpoint.connect_to(addr)));

        let (conn, incoming_messages) = future::select_ok(tasks)
            .await
            .map_err(|err| {
                error!("Failed to bootstrap to the network: {}", err);
                Error::BootstrapFailure
            })?
            .0;

        // NOTE: this error is impossible because we just created new endpoint so the connection
        // cannot be in the pool yet and thus `incoming_messages` should be `Some`. But handling it
        // anyway to avoid `unwrap`.
        let incoming_messages = incoming_messages.ok_or(Error::BootstrapFailure)?;

        Ok((endpoint, conn, incoming_messages))
    }

    /// Connect to the given peer and return the `Endpoint` created along with the `Connection`
    /// object if it succeeds, which can then be used to send messages to the connected peer.
    ///
    /// # Example
    ///
    /// ```
    /// use qp2p::{QuicP2p, Config, Error};
    /// use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Error> {
    ///
    ///     let mut config = Config::default();
    ///     config.ip = Some(IpAddr::V4(Ipv4Addr::LOCALHOST));
    ///     let mut quic_p2p = QuicP2p::with_config(Some(config.clone()), Default::default(), true)?;
    ///     let peer_1 = quic_p2p.new_endpoint()?;
    ///     let peer1_addr = peer_1.socket_addr().await?;
    ///
    ///     let (peer_2, connection) = quic_p2p.connect_to(&peer1_addr).await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn connect_to(&self, node_addr: &SocketAddr) -> Result<(Endpoint, Connection)> {
        let endpoint = self.new_endpoint()?;
        let (conn, _) = endpoint.connect_to(node_addr).await?;

        Ok((endpoint, conn))
    }

    /// Create a new `Endpoint`  which can be used to connect to peers and send
    /// messages to them, as well as listen to messages incoming from other peers.
    ///
    /// # Example
    ///
    /// ```
    /// use qp2p::{QuicP2p, Config, Error};
    /// use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    /// #[tokio::main]
    /// async fn main() -> Result<(), Error> {
    ///
    ///     let mut config = Config::default();
    ///     config.ip = Some(IpAddr::V4(Ipv4Addr::LOCALHOST));
    ///     let mut quic_p2p = QuicP2p::with_config(Some(config.clone()), Default::default(), true)?;
    ///     let endpoint = quic_p2p.new_endpoint()?;
    ///     Ok(())
    /// }
    /// ```
    pub fn new_endpoint(&self) -> Result<Endpoint> {
        trace!("Creating a new endpoint");

        let bootstrap_nodes: Vec<SocketAddr> = self
            .bootstrap_cache
            .peers()
            .iter()
            .rev()
            .chain(self.bootstrap_cache.hard_coded_contacts().iter())
            .cloned()
            .collect();

        let (quinn_endpoint, quinn_incoming) = bind(
            self.endpoint_cfg.clone(),
            self.local_addr,
            self.allow_random_port,
        )?;

        trace!("Bound endpoint to local address: {}", self.local_addr);

        let endpoint = Endpoint::new(
            quinn_endpoint,
            quinn_incoming,
            self.client_cfg.clone(),
            bootstrap_nodes,
            self.qp2p_config.clone(),
        )?;

        Ok(endpoint)
    }
}

// Private helpers

// Bind a new socket with a local address
fn bind(
    endpoint_cfg: quinn::ServerConfig,
    local_addr: SocketAddr,
    allow_random_port: bool,
) -> Result<(quinn::Endpoint, quinn::Incoming)> {
    let mut endpoint_builder = quinn::Endpoint::builder();
    let _ = endpoint_builder.listen(endpoint_cfg);

    match UdpSocket::bind(&local_addr) {
        Ok(udp) => endpoint_builder.with_socket(udp).map_err(Error::Endpoint),
        Err(err) if allow_random_port => {
            info!(
                "Failed to bind to local address: {} - Error: {}. Trying random port instead.",
                local_addr, err
            );
            let bind_addr = SocketAddr::new(local_addr.ip(), 0);

            endpoint_builder.bind(&bind_addr).map_err(|e| {
                error!("Failed to bind to random port too: {}", e);
                Error::Endpoint(e)
            })
        }
        Err(err) => Err(Error::Configuration(format!(
            "Could not bind to the user supplied port: {}! Error: {}",
            local_addr.port(),
            err
        ))),
    }
}

fn unwrap_config_or_default(cfg: Option<Config>) -> Result<Config> {
    let mut cfg = cfg.map_or(Config::read_or_construct_default(None)?, |cfg| cfg);
    if cfg.ip.is_none() && cfg.forward_port {
        cfg.ip = crate::igd::get_local_ip().ok();
    };
    if cfg.clean {
        Config::clear_config_from_disk(None)?;
    }
    Ok(cfg)
}
