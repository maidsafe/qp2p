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
    peer_config::{self, DEFAULT_IDLE_TIMEOUT_MSEC, DEFAULT_KEEP_ALIVE_INTERVAL_MSEC},
    utils::R,
    wire_msg::{Handshake, WireMsg},
};
use bytes::Bytes;
use futures::future::select_ok;
use futures::stream::StreamExt;
use log::{error, info, trace, warn};
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
#[derive(Clone)]
pub struct QuicP2p {
    local_addr: SocketAddr,
    allow_random_port: bool,
    max_msg_size: usize,
    bootstrap_cache: BootstrapCache,
    endpoint_cfg: quinn::ServerConfig,
    client_cfg: quinn::ClientConfig,
    our_type: OurType,
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

        let our_type = cfg.our_type;

        let (port, allow_random_port) = cfg
            .port
            .map(|p| (p, false))
            .unwrap_or((DEFAULT_PORT_TO_TRY, true));

        let ip = cfg.ip.unwrap_or_else(|| IpAddr::V4(Ipv4Addr::UNSPECIFIED));

        let max_msg_size = cfg
            .max_msg_size_allowed
            .map(|size| size as usize)
            .unwrap_or(DEFAULT_MAX_ALLOWED_MSG_SIZE);

        let idle_timeout_msec = cfg.idle_timeout_msec.unwrap_or(DEFAULT_IDLE_TIMEOUT_MSEC);

        let keep_alive_interval_msec = cfg
            .keep_alive_interval_msec
            .unwrap_or(DEFAULT_KEEP_ALIVE_INTERVAL_MSEC);

        let (key, cert) = {
            let our_complete_cert: SerialisableCertificate = Default::default();
            our_complete_cert.obtain_priv_key_and_cert()?
        };

        let custom_dirs = cfg
            .bootstrap_cache_dir
            .clone()
            .map(|custom_dir| Dirs::Overide(OverRide::new(&custom_dir)));

        let mut bootstrap_cache =
            BootstrapCache::new(cfg.hard_coded_contacts, custom_dirs.as_ref())?;
        if use_bootstrap_cache {
            bootstrap_cache
                .peers_mut()
                .extend(bootstrap_nodes.into_iter());
        } else {
            let _ = mem::replace(bootstrap_cache.peers_mut(), bootstrap_nodes);
        }

        let endpoint_cfg =
            peer_config::new_our_cfg(idle_timeout_msec, keep_alive_interval_msec, cert, key)?;

        let client_cfg = peer_config::new_client_cfg(idle_timeout_msec, keep_alive_interval_msec);

        let quic_p2p = Self {
            local_addr: SocketAddr::new(ip, port),
            allow_random_port,
            max_msg_size,
            bootstrap_cache,
            endpoint_cfg,
            client_cfg,
            our_type,
        };

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
        // TODO: refactor bootstrap_cache so we can simply get the list of nodes
        let bootstrap_nodes: Vec<SocketAddr> = self
            .bootstrap_cache
            .peers()
            .iter()
            .rev()
            .chain(self.bootstrap_cache.hard_coded_contacts().iter())
            .cloned()
            .collect();

        trace!("Bootstrapping with nodes {:?}", bootstrap_nodes);
        // Attempt to connect to all nodes and return the first one to succeed
        let mut tasks = Vec::default();
        for node_addr in bootstrap_nodes {
            let endpoint_cfg = self.endpoint_cfg.clone();
            let client_cfg = self.client_cfg.clone();
            let max_msg_size = self.max_msg_size;
            let our_type = self.our_type;
            let local_addr = self.local_addr;
            let allow_random_port = self.allow_random_port;
            let task_handle = tokio::spawn(async move {
                new_connection_to(
                    node_addr,
                    endpoint_cfg,
                    client_cfg,
                    max_msg_size,
                    our_type,
                    local_addr,
                    allow_random_port,
                )
                .await
            });
            tasks.push(task_handle);
        }

        let (connection, _) = select_ok(tasks).await.map_err(|err| {
            error!("Failed to botstrap to the network: {}", err);
            QuicP2pError::BootstrapFailure
        })?;

        connection
    }

    /// Connect to the given peer and return a `Connection` object if it succeeds,
    /// which can then be used to send messages to the connected peer.
    pub async fn connect_to(&mut self, node_addr: SocketAddr) -> R<Connection> {
        new_connection_to(
            node_addr,
            self.endpoint_cfg.clone(),
            self.client_cfg.clone(),
            self.max_msg_size,
            self.our_type,
            self.local_addr,
            self.allow_random_port,
        )
        .await
    }

    /// Obtain stream of incoming QUIC connections
    pub fn listen(&self) -> R<IncomingConnections> {
        IncomingConnections::new(
            self.endpoint_cfg.clone(),
            self.local_addr,
            self.allow_random_port,
            self.max_msg_size,
        )
    }
}

// Creates a new Connection
async fn new_connection_to(
    node_addr: SocketAddr,
    endpoint_cfg: quinn::ServerConfig,
    client_cfg: quinn::ClientConfig,
    max_msg_size: usize,
    our_type: OurType,
    local_addr: SocketAddr,
    allow_random_port: bool,
) -> R<Connection> {
    trace!("Attempting to connect to peer: {}", node_addr);
    let (quinn_endpoint, _) = bind(endpoint_cfg, local_addr, allow_random_port)?;

    let quinn_connecting = quinn_endpoint.connect_with(client_cfg, &node_addr, CERT_SERVER_NAME)?;

    let quinn::NewConnection {
        connection: quic_conn,
        uni_streams,
        ..
    } = quinn_connecting.await?;

    trace!("Successfully connected to peer: {}", node_addr);

    Connection::new(quic_conn, uni_streams, max_msg_size, our_type).await
}

// Bind a new socket with a local address
fn bind(
    endpoint_cfg: quinn::ServerConfig,
    local_addr: SocketAddr,
    allow_random_port: bool,
) -> R<(quinn::Endpoint, quinn::Incoming)> {
    let mut endpoint_builder = quinn::Endpoint::builder();
    let _ = endpoint_builder.listen(endpoint_cfg);

    match UdpSocket::bind(&local_addr) {
        Ok(udp) => endpoint_builder
            .with_socket(udp)
            .map_err(QuicP2pError::Endpoint),
        Err(err) if allow_random_port => {
            info!(
                "Failed to bind to port: {} - Error: {}. Trying random port instead.",
                DEFAULT_PORT_TO_TRY, err
            );
            let bind_addr = SocketAddr::new(local_addr.ip(), 0);
            endpoint_builder
                .bind(&bind_addr)
                .map_err(QuicP2pError::Endpoint)
        }
        Err(err) => Err(QuicP2pError::Configuration {
            e: format!(
                "Could not bind to the user supplied port: {}! Error: {}",
                local_addr.port(),
                err
            ),
        }),
    }
}

/// Connection instance to a node which can be used to send messages to it
pub struct Connection {
    quic_conn: quinn::Connection,
    uni_streams: quinn::IncomingUniStreams,
    max_msg_size: usize,
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.quic_conn.close(0u32.into(), b"");
    }
}

impl Connection {
    pub(crate) async fn new(
        quic_conn: quinn::Connection,
        uni_streams: quinn::IncomingUniStreams,
        max_msg_size: usize,
        our_type: OurType,
    ) -> R<Self> {
        let conn = Self {
            quic_conn,
            uni_streams,
            max_msg_size,
        };
        conn.send_handshake(our_type).await?;

        Ok(conn)
    }

    /// Send message to peer and await for a reponse.
    pub async fn send(&mut self, msg: Bytes) -> R<Bytes> {
        let send_stream = self.quic_conn.open_uni().await?;
        let wire_msg = WireMsg::UserMsg(msg);
        self.send_wire_msg(send_stream, wire_msg).await?;

        trace!(
            "Awaiting for response from remote peer: {}",
            self.quic_conn.remote_address()
        );

        // Let's read the response (we expect one single message)
        match self.uni_streams.next().await {
            None => Err(QuicP2pError::ResponseNotReceived),
            Some(Err(e)) => Err(QuicP2pError::Connection(e)),
            Some(Ok(recv_stream)) => {
                let received_bytes = recv_stream.read_to_end(self.max_msg_size).await?;
                trace!(
                    "Received {} bytes from remote peer: {}",
                    received_bytes.len(),
                    self.quic_conn.remote_address()
                );

                match WireMsg::from_raw(received_bytes)? {
                    WireMsg::UserMsg(msg_bytes) => Ok(Bytes::copy_from_slice(&msg_bytes)),
                    msg => {
                        warn!("Unexpected message type received: {:?}", msg);
                        Err(QuicP2pError::UnexpectedMessageType)
                    }
                }
            }
        }
    }

    /// Send message to peer without awaiting for a reponse.
    pub async fn send_only(&self, msg: Bytes) -> R<()> {
        let send_stream = self.quic_conn.open_uni().await?;
        let wire_msg = WireMsg::UserMsg(msg);
        self.send_wire_msg(send_stream, wire_msg).await
    }

    pub(crate) async fn send_handshake(&self, our_type: OurType) -> R<()> {
        // TODO: try to either have a single client/node type, or remove the need of such info
        let wire_msg = match our_type {
            OurType::Client => WireMsg::Handshake(Handshake::Client),
            OurType::Node => WireMsg::Handshake(Handshake::Node),
        };
        let send_stream = self.quic_conn.open_uni().await?;
        self.send_wire_msg(send_stream, wire_msg).await
    }

    // Private helper to send bytes to peer using the provided stream.
    async fn send_wire_msg(&self, mut send_stream: quinn::SendStream, wire_msg: WireMsg) -> R<()> {
        // TODO: review if we need to use the WireMsg struct at all

        // Let's generate the message bytes
        let (msg_bytes, msg_flag) = wire_msg.into();

        trace!(
            "Sending message to remote peer ({} bytes): {}",
            msg_bytes.len(),
            self.quic_conn.remote_address()
        );

        // Send message bytes over QUIC
        send_stream.write_all(&msg_bytes[..]).await?;

        // Then send message flag over QUIC
        send_stream.write_all(&[msg_flag]).await?;

        send_stream.finish().await?;

        trace!(
            "Message was sent to remote peer: {}",
            self.quic_conn.remote_address(),
        );

        Ok(())
    }
}

/// Stream of incoming QUIC connections
pub struct IncomingConnections {
    quinn_incoming: quinn::Incoming,
    max_msg_size: usize,
}

impl IncomingConnections {
    pub(crate) fn new(
        endpoint_cfg: quinn::ServerConfig,
        local_addr: SocketAddr,
        allow_random_port: bool,
        max_msg_size: usize,
    ) -> R<Self> {
        let (_, quinn_incoming) = bind(endpoint_cfg, local_addr, allow_random_port)?;

        Ok(Self {
            quinn_incoming,
            max_msg_size,
        })
    }

    // Returns next QUIC connection established by a peer
    pub async fn next(&mut self) -> Option<IncomingMessages> {
        match self.quinn_incoming.next().await {
            Some(quinn_conn) => match quinn_conn.await {
                Ok(quinn::NewConnection { bi_streams, .. }) => {
                    Some(IncomingMessages::new(bi_streams, self.max_msg_size))
                }
                Err(_err) => None,
            },
            None => None,
        }
    }
}

/// Stream of incoming QUIC messages
pub struct IncomingMessages {
    bi_streams: quinn::IncomingBiStreams,
    max_msg_size: usize,
}

impl IncomingMessages {
    pub(crate) fn new(bi_streams: quinn::IncomingBiStreams, max_msg_size: usize) -> Self {
        Self {
            bi_streams,
            max_msg_size,
        }
    }

    // Returns next message sent by the peer on current QUIC connection
    pub async fn next(&mut self) -> Option<Bytes> {
        // Each stream initiated by the remote peer constitutes a new message
        match self.bi_streams.next().await {
            None => None,
            Some(stream) => {
                let (_send, recv): (quinn::SendStream, quinn::RecvStream) = match stream {
                    Err(quinn::ConnectionError::ApplicationClosed { .. }) => {
                        trace!("Connection terminated by peer.");
                        return None;
                    }
                    Err(err) => {
                        warn!("Failed to read incoming message: {}", err);
                        return None;
                    }
                    Ok(bi_stream) => bi_stream,
                };

                // Read the message's bytes which size is capped
                match recv.read_to_end(self.max_msg_size).await {
                    Ok(req_bytes) => {
                        trace!("Got new message with {} bytes.", req_bytes.len());
                        /*match parse_jsonrpc_request(req_bytes) {
                            Ok(jsonrpc_req) => {
                                debug!("Request parsed successfully");
                                Some((jsonrpc_req, JsonRpcResponseStream::new(send)))
                            }
                            Err(err) => {
                                warn!("Failed to parse request as JSON-RPC: {}", err);
                                None
                            }
                        }*/
                        None
                    }
                    Err(err) => {
                        warn!("Failed reading message's bytes: {}", err);
                        None
                    }
                }
            }
        }
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
