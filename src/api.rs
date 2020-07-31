// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

#[cfg(feature = "upnp")]
use super::igd;
use super::{
    bootstrap_cache::BootstrapCache,
    config::{Config, OurType, SerialisableCertificate},
    dirs::{Dirs, OverRide},
    error::QuicP2pError,
    listener,
    peer_config::{self, DEFAULT_IDLE_TIMEOUT_MSEC, DEFAULT_KEEP_ALIVE_INTERVAL_MSEC},
    utils::R,
    wire_msg::WireMsg,
};
use bytes::Bytes;
use log::{error, info, trace};
use std::{
    collections::VecDeque,
    mem,
    net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
};

/// Default maximum allowed message size. We'll error out on any bigger messages and probably
/// shutdown the connection. This value can be overridden via the `Config` option.
pub const DEFAULT_MAX_ALLOWED_MSG_SIZE: usize = 500 * 1024 * 1024; // 500MiB

/// In the absence of a port supplied by the user via the config we will first try using this
/// before using a random port.
pub const DEFAULT_PORT_TO_TRY: u16 = 12000;

/// Host name of the Quic communication certificate used by peers
// TODO: make it configurable
const CERT_SERVER_NAME: &str = "MaidSAFE.net";

/// Main QuicP2p instance to communicate with QuicP2p using an async API
pub struct QuicP2p {
    //public_addr: Option<SocketAddr>
    max_msg_size_allowed: usize,
    //our_type: OurType,
    bootstrap_cache: BootstrapCache,
    quic_endpoint: quinn::Endpoint,
    quic_client_cfg: quinn::ClientConfig,
}

impl QuicP2p {
    /// Construct `QuicP2p` with the default config and bootstrap cache enabled
    pub fn new() -> R<Self> {
        Self::with_config(None, Default::default(), true)
    }

    /// Construct `QuicP2p` with supplied parameters, ready to be used.
    /// If config is not specified it'll call `Config::read_or_construct_default()`
    ///
    /// `bootstrap_nodes`: takes bootstrap nodes from the user.
    ///
    /// In addition to bootstrap nodes provided, optionally use the nodes found
    /// in the bootstrap cache file (if such a file exists) or disable this feature.
    pub fn with_config(
        cfg: Option<Config>,
        bootstrap_nodes: VecDeque<SocketAddr>,
        use_bootstrap_cache: bool,
    ) -> R<Self> {
        let cfg = unwrap_config_or_default(cfg)?;

        let (port, is_user_supplied) = cfg
            .port
            .map(|p| (p, true))
            .unwrap_or((DEFAULT_PORT_TO_TRY, false));

        let ip = cfg.ip.unwrap_or_else(|| IpAddr::V4(Ipv4Addr::UNSPECIFIED));

        let max_msg_size_allowed = cfg
            .max_msg_size_allowed
            .map(|size| size as usize)
            .unwrap_or(DEFAULT_MAX_ALLOWED_MSG_SIZE);

        let idle_timeout_msec = cfg.idle_timeout_msec.unwrap_or(DEFAULT_IDLE_TIMEOUT_MSEC);

        let keep_alive_interval_msec = cfg
            .keep_alive_interval_msec
            .unwrap_or(DEFAULT_KEEP_ALIVE_INTERVAL_MSEC);

        let our_type = cfg.our_type;

        let hard_coded_contacts = cfg.hard_coded_contacts.clone();

        let (key, cert) = {
            let our_complete_cert: SerialisableCertificate = Default::default();
            our_complete_cert.obtain_priv_key_and_cert()?
        };

        let custom_dirs = cfg
            .bootstrap_cache_dir
            .clone()
            .map(|custom_dir| Dirs::Overide(OverRide::new(&custom_dir)));

        let mut bootstrap_cache = BootstrapCache::new(hard_coded_contacts, custom_dirs.as_ref())?;
        if use_bootstrap_cache {
            bootstrap_cache
                .peers_mut()
                .extend(bootstrap_nodes.into_iter());
        } else {
            let _ = mem::replace(bootstrap_cache.peers_mut(), bootstrap_nodes);
        }

        let quinn_config =
            peer_config::new_our_cfg(idle_timeout_msec, keep_alive_interval_msec, cert, key)
                .unwrap();

        let mut ep_builder = quinn::Endpoint::builder();
        let _ = ep_builder.listen(quinn_config);
        let (quic_endpoint, incoming_connections) = {
            match UdpSocket::bind(&(ip, port)) {
                Ok(udp) => ep_builder.with_socket(udp).unwrap(),
                Err(e) => {
                    if is_user_supplied {
                        panic!(
                            "Could not bind to the user supplied port: {}! Error: {:?}- {}",
                            port, e, e
                        );
                    }
                    info!(
                        "Failed to bind to port: {} - Error: {:?} - {}. Trying random port.",
                        DEFAULT_PORT_TO_TRY, e, e
                    );
                    let bind_addr = SocketAddr::new(ip, 0);
                    ep_builder.bind(&bind_addr).unwrap()
                }
            }
        };

        let quic_client_cfg =
            peer_config::new_client_cfg(idle_timeout_msec, keep_alive_interval_msec).unwrap();

        let quic_p2p = Self {
            max_msg_size_allowed,
            //our_type,
            bootstrap_cache,
            quic_endpoint,
            quic_client_cfg,
        };

        if our_type != OurType::Client {
            listener::listen(incoming_connections);
        }

        Ok(quic_p2p)
    }

    /// Bootstrap to the network.
    ///
    /// Bootstrap concept is different from "connect" in several ways: `bootstrap()` will try to
    /// connect to all peers which are specified in the config (`hard_coded_contacts`) or were
    /// previously cached.
    /// Once a connection with a peer succeeds, a `Connection` for such peer will be returned
    /// and all other connections will be dropped.
    pub async fn bootstrap(&mut self) -> R<Connection> {
        let bootstrap_nodes: Vec<SocketAddr> = self
            .bootstrap_cache
            .peers()
            .iter()
            .rev()
            .chain(self.bootstrap_cache.hard_coded_contacts().iter())
            .cloned()
            .collect();

        trace!("Bootstrapping to {:?}", bootstrap_nodes);

        for node_addr in bootstrap_nodes {
            // TODO: attempt to connect to all and return the first one to succeed
            return self.connect_to(node_addr).await;
        }

        error!("Failed to botstrap to the network");
        Err(QuicP2pError::BootstrapFailure)
    }

    /// Connect to the given peer and return a `Connection` object if it succeeds,
    /// which can then be used to send messages to the connected peer.
    pub async fn connect_to(&mut self, node_addr: SocketAddr) -> R<Connection> {
        let quinn_connecting = self.quic_endpoint.connect_with(
            self.quic_client_cfg.clone(),
            &node_addr,
            CERT_SERVER_NAME,
        )?;

        let quinn::NewConnection {
            connection: quic_conn,
            ..
        } = quinn_connecting.await?;

        trace!("Successfully connected to peer: {}", node_addr);

        Ok(Connection {
            quic_conn,
            max_msg_size_allowed: self.max_msg_size_allowed,
        })
    }
}

/// Connection instance to a node which can be used to send messages to it
pub struct Connection {
    quic_conn: quinn::Connection,
    max_msg_size_allowed: usize,
}

impl Connection {
    /// Send message to peer.
    ///
    /// If the peer is not connected, it will attempt to connect to it first
    /// and then send the message. This can be called multiple times while the peer is still being
    /// connected to - all the sends will be buffered until the peer is connected to.
    pub async fn send(&self, msg: Bytes) -> R<Bytes> {
        // TODO: review if we try to reconnect or simply fail if it lost connection
        // TODO: review if we need to use the WireMsg struct at all

        // Let's generate the request bytes
        let (msg_bytes, msg_flag) = WireMsg::UserMsg(msg).into();

        let (mut send, recv) = self.quic_conn.open_bi().await?;

        // Send request bytes over QUIC
        send.write_all(&msg_bytes[..]).await?;

        // Then send request flaag over QUIC
        send.write_all(&[msg_flag]).await?;

        send.finish().await?;

        trace!("Request was sent to remote peer");

        // Let's await to read the response
        let received_bytes = recv.read_to_end(self.max_msg_size_allowed).await?;

        self.quic_conn.close(0u32.into(), b"");

        Ok(Bytes::copy_from_slice(&received_bytes))
    }
}

// Private helpers

// Unwrap the conffig if provided by the user, otherwise construct the default one
#[cfg(not(feature = "upnp"))]
fn unwrap_config_or_default(cfg: Option<Config>) -> R<Config> {
    cfg.map_or(Config::read_or_construct_default(None), |cfg| Ok(cfg))
}

#[cfg(feature = "upnp")]
fn unwrap_config_or_default(cfg: Option<Config>) -> R<Config> {
    let mut cfg = cfg.map_or(Config::read_or_construct_default(None)?, |cfg| cfg);
    if cfg.ip.is_none() {
        cfg.ip = igd::get_local_ip().ok();
    };

    Ok(cfg)
}
