// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

//! quic-p2p enables communication within a peer to peer network over the QUIC protocol.

// For explanation of lint checks, run `rustc -W help`
#![forbid(
    exceeding_bitshifts,
    mutable_transmutes,
    no_mangle_const_items,
    unknown_crate_types,
    warnings
)]
#![deny(
    bad_style,
    deprecated,
    improper_ctypes,
    missing_docs,
    non_shorthand_field_patterns,
    overflowing_literals,
    stable_features,
    unconditional_recursion,
    unknown_lints,
    unsafe_code,
    unused,
    unused_allocation,
    unused_attributes,
    unused_comparisons,
    unused_features,
    unused_parens,
    while_true,
    clippy::unicode_not_nfc,
    clippy::wrong_pub_self_convention,
    clippy::option_unwrap_used
)]
#![warn(
    trivial_casts,
    trivial_numeric_casts,
    unused_extern_crates,
    unused_import_braces,
    unused_qualifications,
    unused_results
)]

pub use config::{Config, OurType, SerialisableCertificate};
pub use dirs::{Dirs, OverRide};
pub use error::QuicP2pError;
pub use event::Event;
pub use peer::Peer;
pub use peer_config::{DEFAULT_IDLE_TIMEOUT_MSEC, DEFAULT_KEEP_ALIVE_INTERVAL_MSEC};
pub use utils::{Token, R};

use crate::wire_msg::WireMsg;
use bootstrap_cache::BootstrapCache;
use bytes::Bytes;
use context::{ctx, ctx_mut, initialise_ctx, Context};
use crossbeam_channel as mpmc;
use event_loop::EventLoop;
use log::{debug, info, warn};
use std::collections::VecDeque;
use std::mem;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::mpsc;
use unwrap::unwrap;

mod bootstrap;
mod bootstrap_cache;
mod communicate;
mod config;
mod connect;
mod connection;
mod context;
mod dirs;
mod error;
mod event;
mod event_loop;
mod listener;
mod peer;
mod peer_config;
#[cfg(test)]
mod test_utils;
mod utils;
mod wire_msg;
/// Default maximum allowed message size. We'll error out on any bigger messages and probably
/// shutdown the connection. This value can be overridden via the `Config` option.
pub const DEFAULT_MAX_ALLOWED_MSG_SIZE: usize = 500 * 1024 * 1024; // 500MiB
/// In the absence of a port supplied by the user via the config we will first try using this
/// before using a random port.
pub const DEFAULT_PORT_TO_TRY: u16 = 443;

/// Senders for node and client events
#[derive(Clone)]
pub struct EventSenders {
    /// The event sender for events comming from clients.
    pub client_tx: mpmc::Sender<Event>,
    /// The event sender for events comming from nodes.
    /// This also includes our own bootstrapping events `Event::BootstrapFailure`
    /// and `Event::BootstrappedTo` as well as `Event::Finish`.
    pub node_tx: mpmc::Sender<Event>,
}

impl EventSenders {
    pub(crate) fn send(&self, event: Event) -> Result<(), mpmc::SendError<Event>> {
        if event.is_node_event() {
            self.node_tx.send(event)
        } else {
            self.client_tx.send(event)
        }
    }
}

/// Builder for `QuicP2p`. Convenient for setting various parameters and creating `QuicP2p`.
pub struct Builder {
    event_tx: EventSenders,
    cfg: Option<Config>,
    bootstrap_nodes: VecDeque<SocketAddr>,
    use_bootstrap_nodes_exclusively: bool,
}

impl Builder {
    /// New `Builder`
    pub fn new(event_tx: EventSenders) -> Self {
        Self {
            event_tx,
            cfg: Default::default(),
            bootstrap_nodes: Default::default(),
            use_bootstrap_nodes_exclusively: Default::default(),
        }
    }

    /// Take bootstrap nodes from the user.
    ///
    /// Either use these exclusively or in addition to the ones read from bootstrap cache file if
    /// such a file exists
    pub fn with_bootstrap_nodes(
        mut self,
        bootstrap_nodes: VecDeque<SocketAddr>,
        use_exclusively: bool,
    ) -> Self {
        self.use_bootstrap_nodes_exclusively = use_exclusively;

        if use_exclusively {
            self.bootstrap_nodes = bootstrap_nodes;
        } else {
            self.bootstrap_nodes.extend(bootstrap_nodes.into_iter());
        }

        self
    }

    /// Configuration for `QuicP2p`.
    ///
    /// If not specified it'll call `Config::read_or_construct_default()`
    pub fn with_config(mut self, cfg: Config) -> Self {
        self.cfg = Some(cfg);
        self
    }

    /// Construct `QuicP2p` with supplied parameters earlier, ready to be used.
    pub fn build(self) -> R<QuicP2p> {
        let mut qp2p = if let Some(cfg) = self.cfg {
            QuicP2p::with_config(self.event_tx, cfg)
        } else {
            QuicP2p::new(self.event_tx)?
        };

        qp2p.activate()?;

        let use_bootstrap_nodes_exclusively = self.use_bootstrap_nodes_exclusively;
        let bootstrap_nodes = self.bootstrap_nodes;

        qp2p.el.post(move || {
            ctx_mut(|c| {
                if use_bootstrap_nodes_exclusively {
                    let _ = mem::replace(c.bootstrap_cache.peers_mut(), bootstrap_nodes);
                } else {
                    c.bootstrap_cache
                        .peers_mut()
                        .extend(bootstrap_nodes.into_iter());
                }
            })
        });

        Ok(qp2p)
    }
}

/// Main QuicP2p instance to communicate with QuicP2p
pub struct QuicP2p {
    event_tx: EventSenders,
    cfg: Config,
    us: Option<SocketAddr>,
    el: EventLoop,
}

impl QuicP2p {
    /// Bootstrap to the network.
    ///
    /// Bootstrap concept is different from "connect" in several ways: `bootstrap()` will try to
    /// connect to all peers which are specified in the config (`hard_coded_contacts`) or were
    /// previously cached. If one bootstrap connection succeeds, all other connections will be dropped.
    ///
    /// In case of success `Event::BootstrapedTo` will be fired. On error quic-p2p will fire `Event::BootstrapFailure`.
    pub fn bootstrap(&mut self) {
        self.el.post(|| {
            bootstrap::start();
        })
    }

    /// Connect to the given peer. This will error out if the peer is already in the process of
    /// being connected to OR for any other connection failure reasons.
    pub fn connect_to(&mut self, node_addr: SocketAddr) {
        self.el.post(move || {
            if let Err(e) = connect::connect_to(node_addr, None, None) {
                info!("Could not connect to the asked peer {}: {}", node_addr, e);
                ctx_mut(|c| {
                    let _ = c.event_tx.send(Event::ConnectionFailure {
                        peer: Peer::Node(node_addr),
                        err: e,
                    });
                });
            } else {
                Self::set_we_contacted_peer(&node_addr);
            }
        });
    }

    /// Disconnect from the given peer
    pub fn disconnect_from(&mut self, peer_addr: SocketAddr) {
        self.el.post(move || {
            ctx_mut(|c| {
                if c.connections.remove(&peer_addr).is_none() {
                    debug!("Asked to disconnect from an unknown peer");
                }
            })
        });
    }

    /// Send message to peer.
    ///
    /// If the peer is not connected, it will attempt to connect to it first
    /// and then send the message. This can be called multiple times while the peer is still being
    /// connected to - all the sends will be buffered until the peer is connected to.
    ///
    /// `token` is supplied by the user code and will be returned with the message to help identify
    /// the context, for successful or unsuccessful sends.
    pub fn send(&mut self, peer: Peer, msg: Bytes, token: Token) {
        self.el.post(move || {
            let peer_addr = peer.peer_addr();

            if let Err(e) =
                communicate::try_write_to_peer(peer.clone(), WireMsg::UserMsg(msg), token)
            {
                info!(
                    "Could not send message to the asked peer {}: {}",
                    peer_addr, e
                );
                ctx_mut(|c| {
                    let _ = c.event_tx.send(Event::ConnectionFailure { peer, err: e });
                });
            } else {
                Self::set_we_contacted_peer(&peer_addr);
            }
        });
    }

    /// Get our connection info to give to others for them to connect to us
    ///
    /// Will use hard coded contacts to ask for our endpoint. If no contact is given then we'll
    /// simply build our connection info by querying the underlying bound socket for our address.
    /// Note that if such an obtained address is of unspecified category we will ignore that as
    /// such an address cannot be reached and hence not useful.
    // FIXME calling this mutliple times concurrently just now could have it hanging as only one tx
    // is registered and that replaces any previous tx registered. Fix by using a vec of txs
    pub fn our_connection_info(&mut self) -> R<SocketAddr> {
        if let Some(us) = self.us {
            return Ok(us);
        }

        let our_addr = match self.query_ip_echo_service() {
            Ok(addr) => addr,
            Err(e @ QuicP2pError::NoEndpointEchoServerFound) => {
                let (tx, rx) = mpsc::channel();
                self.el.post(move || {
                    let local_addr_res = ctx(|c| c.quic_ep().local_addr());
                    unwrap!(tx.send(local_addr_res));
                });
                let addr = unwrap!(rx.recv())?;
                if addr.ip().is_unspecified() {
                    return Err(e);
                } else {
                    addr
                }
            }
            Err(e) => return Err(e),
        };

        self.us = Some(our_addr);

        Ok(our_addr)
    }

    /// Retrieves current node bootstrap cache.
    pub fn bootstrap_cache(&mut self) -> R<Vec<SocketAddr>> {
        let (tx, rx) = mpsc::channel();
        self.el.post(move || {
            let cache = ctx(|c| c.bootstrap_cache.peers().iter().cloned().collect());
            let _ = tx.send(cache);
        });
        let cache = rx.recv()?;

        Ok(cache)
    }

    /// Checks whether the given contact is hard-coded.
    pub fn is_hard_coded_contact(&self, node_addr: &SocketAddr) -> bool {
        self.cfg.hard_coded_contacts.contains(node_addr)
    }

    /// Returns a copy of the `Config` used to create this `QuicP2p`.
    pub fn config(&self) -> Config {
        self.cfg.clone()
    }

    fn new(event_tx: EventSenders) -> R<Self> {
        Ok(Self::with_config(
            event_tx,
            Config::read_or_construct_default(None)?,
        ))
    }

    fn with_config(event_tx: EventSenders, cfg: Config) -> Self {
        let el = EventLoop::spawn();
        Self {
            event_tx,
            cfg,
            us: None,
            el,
        }
    }

    /// Must be called only once. There can only be one context per `QuicP2p` instance.
    fn activate(&mut self) -> R<()> {
        let (port, is_user_supplied) = self
            .cfg
            .port
            .map(|p| (p, true))
            .unwrap_or((DEFAULT_PORT_TO_TRY, false));
        let ip = self
            .cfg
            .ip
            .unwrap_or_else(|| IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        let max_msg_size_allowed = self
            .cfg
            .max_msg_size_allowed
            .map(|size| size as usize)
            .unwrap_or(DEFAULT_MAX_ALLOWED_MSG_SIZE);
        let idle_timeout_msec = self
            .cfg
            .idle_timeout_msec
            .unwrap_or(DEFAULT_IDLE_TIMEOUT_MSEC);
        let keep_alive_interval_msec = self
            .cfg
            .keep_alive_interval_msec
            .unwrap_or(DEFAULT_KEEP_ALIVE_INTERVAL_MSEC);
        let our_type = self.cfg.our_type;
        let hard_coded_contacts = self.cfg.hard_coded_contacts.clone();

        let tx = self.event_tx.clone();

        let ((key, cert), our_complete_cert) = {
            let our_complete_cert = self
                .cfg
                .our_complete_cert
                .clone()
                .unwrap_or_else(Default::default);
            (
                our_complete_cert.obtain_priv_key_and_cert()?,
                our_complete_cert,
            )
        };
        let custom_dirs = self
            .cfg
            .bootstrap_cache_dir
            .clone()
            .map(|custom_dir| Dirs::Overide(OverRide::new(&custom_dir)));
        let bootstrap_cache = BootstrapCache::new(hard_coded_contacts, custom_dirs.as_ref())?;

        self.el.post(move || {
            let our_cfg = unwrap!(peer_config::new_our_cfg(
                idle_timeout_msec,
                keep_alive_interval_msec,
                cert,
                key
            ));

            let mut ep_builder = quinn::Endpoint::builder();
            let _ = ep_builder.listen(our_cfg);
            let (ep, incoming_connections) = {
                match UdpSocket::bind(&(ip, port)) {
                    Ok(udp) => unwrap!(ep_builder.with_socket(udp)),
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
                        unwrap!(ep_builder.bind(&bind_addr))
                    }
                }
            };

            let client_cfg =
                peer_config::new_client_cfg(idle_timeout_msec, keep_alive_interval_msec).unwrap();

            let ctx = Context::new(
                tx,
                our_complete_cert,
                max_msg_size_allowed,
                our_type,
                bootstrap_cache,
                ep,
                client_cfg,
            );
            initialise_ctx(ctx);

            if our_type != OurType::Client {
                listener::listen(incoming_connections);
            }
        });

        Ok(())
    }

    fn query_ip_echo_service(&mut self) -> R<SocketAddr> {
        // FIXME: For the purpose of simplicity we are asking only one peer just now. In production
        // ask multiple until one answers OR we exhaust the list
        let node_addr = if let Some(node_addr) = self.cfg.hard_coded_contacts.iter().next() {
            *node_addr
        } else {
            return Err(QuicP2pError::NoEndpointEchoServerFound);
        };
        let echo_server = Peer::Node(node_addr);

        let (tx, rx) = mpsc::channel();

        self.el.post(move || {
            ctx_mut(|c| c.our_ext_addr_tx = Some(tx));
            let _ = communicate::try_write_to_peer(echo_server, WireMsg::EndpointEchoReq, 0);
        });

        Ok(unwrap!(rx.recv()))
    }

    #[inline]
    fn set_we_contacted_peer(peer_addr: &SocketAddr) {
        ctx_mut(|c| {
            if let Some(conn) = c.connections.get_mut(peer_addr) {
                conn.we_contacted_peer = true;
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire_msg::{Handshake, WireMsg};
    use crossbeam_channel as mpmc;
    use std::collections::HashSet;
    use std::iter;
    use std::time::Duration;
    use test_utils::{new_random_qp2p, new_unbounded_channels, rand_node_addr};

    #[ignore] // This fails on fast machines FIXME
    #[test]
    fn dropping_qp2p_handle_gracefully_shutsdown_event_loop() {
        let (tx, _rx) = new_unbounded_channels();
        let _qp2p = unwrap!(Builder::new(tx).build());
    }

    #[test]
    fn echo_service() {
        let (mut qp2p0, _rx) = new_random_qp2p(false, Default::default());

        // Confirm there's no echo service available for us
        match qp2p0.query_ip_echo_service() {
            Ok(_) => panic!("Without Hard Coded Contacts, echo service should not be possible"),
            Err(QuicP2pError::NoEndpointEchoServerFound) => (),
            Err(e) => panic!("{:?} - {}", e, e),
        }

        // Now the only way to obtain info is via querring the quic_ep for the bound address
        let qp2p0_info = unwrap!(qp2p0.our_connection_info());
        let qp2p0_port = qp2p0_info.port();

        let (mut qp2p1, rx1) = {
            let mut hcc: HashSet<_> = Default::default();
            assert!(hcc.insert(qp2p0_info.clone()));
            new_random_qp2p(true, hcc)
        };

        // Echo service is availabe through qp2p0
        let qp2p1_port = unwrap!(qp2p1.query_ip_echo_service()).port();
        let qp2p1_info = unwrap!(qp2p1.our_connection_info());

        assert_ne!(qp2p0_port, qp2p1_port);
        assert_eq!(qp2p1_port, qp2p1_info.port());

        let (mut qp2p2, rx2) = {
            let mut hcc: HashSet<_> = Default::default();
            assert!(hcc.insert(qp2p0_info));
            new_random_qp2p(true, hcc)
        };
        let qp2p2_info = unwrap!(qp2p2.our_connection_info());

        // The two qp2p can now send data to each other
        // Drain the receiver first
        while let Ok(_) = rx1.try_recv() {}
        while let Ok(_) = rx2.try_recv() {}

        let data = bytes::Bytes::from(vec![12, 13, 14, 253]);
        const TOKEN: u64 = 19923;
        qp2p2.send(Peer::Node(qp2p1_info), data.clone(), TOKEN);

        match unwrap!(rx1.recv()) {
            Event::ConnectedTo { peer } => assert_eq!(peer, Peer::Node(qp2p2_info)),
            x => panic!("Received unexpected event: {:?}", x),
        }
        match unwrap!(rx1.recv()) {
            Event::NewMessage { peer, msg } => {
                assert_eq!(peer.peer_addr(), qp2p2_info);
                assert_eq!(msg, data);
            }
            x => panic!("Received unexpected event: {:?}", x),
        }
        match unwrap!(rx2.recv()) {
            Event::SentUserMessage { peer, msg, token } => {
                assert_eq!(peer.peer_addr(), qp2p1_info);
                assert_eq!(msg, data);
                assert_eq!(token, TOKEN);
            }
            x => panic!("Received unexpected event: {:?}", x),
        }
    }

    #[test]
    fn multistreaming_and_no_head_of_queue_blocking() {
        const TEST_TOKEN0: u64 = 293_203;
        const TEST_TOKEN1: u64 = 3_435_235;

        let (mut qp2p0, rx0) = new_random_qp2p(false, Default::default());
        let qp2p0_addr = unwrap!(qp2p0.our_connection_info());

        let (mut qp2p1, rx1) = {
            let mut hcc: HashSet<_> = Default::default();
            assert!(hcc.insert(qp2p0_addr));
            new_random_qp2p(true, hcc)
        };
        let qp2p1_addr = unwrap!(qp2p1.our_connection_info());

        // 400 MiB message
        let big_msg_to_qp2p0 = bytes::Bytes::from(vec![255; 400 * 1024 * 1024]);
        let big_msg_to_qp2p0_clone = big_msg_to_qp2p0.clone();

        // very small messages
        let small_msg0_to_qp2p0 = bytes::Bytes::from(vec![255, 254, 253, 252]);
        let small_msg0_to_qp2p0_clone = small_msg0_to_qp2p0.clone();

        let small_msg1_to_qp2p0 = bytes::Bytes::from(vec![155, 154, 153, 152]);
        let small_msg1_to_qp2p0_clone = small_msg1_to_qp2p0.clone();

        let msg_to_qp2p1 = bytes::Bytes::from(vec![120, 129, 2]);
        let msg_to_qp2p1_clone0 = msg_to_qp2p1.clone();
        let msg_to_qp2p1_clone1 = msg_to_qp2p1.clone();

        let j0 = unwrap!(std::thread::Builder::new()
            .name("QuicP2p0-test-thread".to_string())
            .spawn(move || {
                match rx0.recv() {
                    Ok(Event::ConnectedTo {
                        peer: Peer::Node(node_addr),
                    }) => assert_eq!(node_addr, qp2p1_addr),
                    Ok(x) => panic!("Expected Event::ConnectedTo - got {:?}", x),
                    Err(e) => panic!(
                        "QuicP2p0 Expected Event::ConnectedTo; got error: {:?} {}",
                        e, e
                    ),
                };
                let mut rxd_sent_msg_event = false;
                for i in 0..4 {
                    match rx0.recv() {
                        Ok(Event::NewMessage { peer, msg }) => {
                            assert_eq!(peer.peer_addr(), qp2p1_addr);
                            if i != 3 {
                                assert!(
                                    msg == small_msg0_to_qp2p0_clone
                                        || msg == small_msg1_to_qp2p0_clone
                                );
                                info!("Smaller message {:?} rxd from {}", &*msg, peer.peer_addr())
                            } else {
                                assert_eq!(msg, big_msg_to_qp2p0_clone);
                                info!(
                                    "Big message of size {} rxd from {}",
                                    msg.len(),
                                    peer.peer_addr()
                                );
                            }
                        }
                        Ok(Event::SentUserMessage { peer, msg, token }) => {
                            if rxd_sent_msg_event {
                                panic!("Should have received sent message event only once !");
                            }
                            assert_eq!(peer.peer_addr(), qp2p1_addr);
                            assert_eq!(msg, msg_to_qp2p1_clone0);
                            assert_eq!(token, TEST_TOKEN0);
                            rxd_sent_msg_event = true;
                        }
                        Ok(x) => panic!("Expected Event::NewMessage - got {:?}", x),
                        Err(e) => panic!(
                            "QuicP2p0 Expected Event::NewMessage; got error: {:?} {}",
                            e, e
                        ),
                    };
                }
            }));
        let j1 = unwrap!(std::thread::Builder::new()
            .name("QuicP2p1-test-thread".to_string())
            .spawn(move || {
                match rx1.recv() {
                    Ok(Event::ConnectedTo {
                        peer: Peer::Node(node_addr),
                    }) => assert_eq!(node_addr, qp2p0_addr),
                    Ok(x) => panic!("Expected Event::ConnectedTo - got {:?}", x),
                    Err(e) => panic!(
                        "QuicP2p1 Expected Event::ConnectedTo; got error: {:?} {}",
                        e, e
                    ),
                };
                let mut count_of_rxd_sent_msgs: u8 = 0;
                for _ in 0..4 {
                match rx1.recv() {
                    Ok(Event::NewMessage { peer, msg }) => {
                        assert_eq!(peer.peer_addr(), qp2p0_addr);
                        assert_eq!(msg, msg_to_qp2p1_clone1);
                    }
                    Ok(Event::SentUserMessage { peer, token, .. }) => {
                        if count_of_rxd_sent_msgs >=3 {
                            panic!("Only sent 3 msgs, so cannot rx send success for more than those
                                   !");
                        }
                        assert_eq!(peer.peer_addr(), qp2p0_addr);
                        assert_eq!(token, TEST_TOKEN1);
                        count_of_rxd_sent_msgs += 1;
                    }
                    Ok(x) => panic!("Expected Event::NewMessage - got {:?}", x),
                    Err(e) => panic!(
                        "QuicP2p1 Expected Event::NewMessage; got error: {:?} {}",
                        e, e
                    ),
                };
                }
            }));

        // Send the biggest message first and we'll assert that it arrives last hence not blocking
        // the rest of smaller messages sent after it
        qp2p1.send(Peer::Node(qp2p0_addr), big_msg_to_qp2p0, TEST_TOKEN1);
        qp2p1.send(Peer::Node(qp2p0_addr), small_msg0_to_qp2p0, TEST_TOKEN1);
        // Even after a delay the following small message should arrive before the 1st sent big
        // message
        std::thread::sleep(Duration::from_millis(100));
        qp2p1.send(Peer::Node(qp2p0_addr), small_msg1_to_qp2p0, TEST_TOKEN1);

        qp2p0.send(Peer::Node(qp2p1_addr), msg_to_qp2p1, TEST_TOKEN0);

        unwrap!(j0.join());
        unwrap!(j1.join());
    }

    // Test for the case when we send an extra handshake introducing ourselves as a Client after we already
    // introduced ourselves as a Node. This message should be just ignored and the peer type should not
    // be changed.
    #[test]
    fn double_handshake_node() {
        let (mut qp2p0, rx0) = new_random_qp2p(false, Default::default());
        let qp2p0_addr = unwrap!(qp2p0.our_connection_info());

        let (tx1, rx1) = new_unbounded_channels();
        let mut malicious_client = unwrap!(Builder::new(tx1)
            .with_config(Config {
                our_type: OurType::Node,
                ip: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
                ..Default::default()
            })
            .build());
        malicious_client.send_wire_msg(
            Peer::Node(qp2p0_addr),
            WireMsg::Handshake(Handshake::Client),
            0,
        );

        let malicious_client_addr = unwrap!(malicious_client.our_connection_info());

        match rx0.recv() {
            Ok(Event::ConnectedTo {
                peer: Peer::Node(node_addr),
            }) => {
                assert_eq!(node_addr, malicious_client_addr);
            }
            r => panic!("Unexpected result {:?}", r),
        }
        match rx1.recv() {
            Ok(Event::ConnectedTo {
                peer: Peer::Node(node_addr),
            }) => {
                assert_eq!(node_addr, qp2p0_addr);
            }
            r => panic!("Unexpected result {:?}", r),
        }

        // No more messages expected
        match rx0.try_recv() {
            Err(mpmc::TryRecvError::Empty) => {}
            r => panic!("Unexpected result {:?}", r),
        }
        match rx1.try_recv() {
            Err(mpmc::TryRecvError::Empty) => {}
            r => panic!("Unexpected result {:?}", r),
        }

        // Check that both have unchanged `ToPeer`/`FromPeer` types.
        let from_peer_is_established =
            unwrap!(malicious_client
                .connections(move |c| { c[&qp2p0_addr].from_peer.is_established() }));
        let to_peer_is_established = unwrap!(
            qp2p0.connections(move |c| { c[&malicious_client_addr].to_peer.is_established() })
        );

        assert!(from_peer_is_established && to_peer_is_established);
    }

    // Test for the case when we send an extra handshake introducing ourselves as a Node after we already
    // introduced ourselves as a Client. This message should be just ignored and the peer type should not
    // be changed.
    #[test]
    fn double_handshake_client() {
        let (mut qp2p0, rx0) = new_random_qp2p(false, Default::default());
        let qp2p0_addr = unwrap!(qp2p0.our_connection_info());

        let (tx1, _rx1) = new_unbounded_channels();
        let mut malicious_client = unwrap!(Builder::new(tx1)
            .with_config(Config {
                our_type: OurType::Client,
                ip: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
                ..Default::default()
            })
            .build());
        malicious_client.send_wire_msg(
            Peer::Node(qp2p0_addr),
            WireMsg::Handshake(Handshake::Node),
            0,
        );

        let malicious_client_addr = unwrap!(malicious_client.our_connection_info());

        match rx0.recv() {
            Ok(Event::ConnectedTo {
                peer: Peer::Client(_),
            }) => {}
            r => panic!("Unexpected result {:?}", r),
        }

        // No more messages expected.
        match rx0.try_recv() {
            Err(mpmc::TryRecvError::Empty) => {}
            r => panic!("Unexpected result {:?}", r),
        }

        // Check that both have unchanged `ToPeer`/`FromPeer` types.
        let from_peer_is_not_needed = unwrap!(
            malicious_client.connections(move |c| { c[&qp2p0_addr].from_peer.is_not_needed() })
        );
        let to_peer_is_not_needed = unwrap!(
            qp2p0.connections(move |c| { c[&malicious_client_addr].to_peer.is_not_needed() })
        );

        assert!(from_peer_is_not_needed && to_peer_is_not_needed);
    }

    #[test]
    fn connect_to_fails() {
        let invalid_socket_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 1);
        let (mut peer, rx) = new_random_qp2p(false, Default::default());
        peer.connect_to(invalid_socket_addr);

        match rx.recv() {
            Ok(Event::ConnectionFailure { peer, .. }) => {
                assert_eq!(peer.peer_addr(), invalid_socket_addr);
            }
            r => panic!("Unexpected result {:?}", r),
        }
    }

    #[test]
    fn connect_to_marks_that_we_attempted_to_contact_the_peer() {
        let (mut peer1, _) = new_random_qp2p(false, Default::default());
        let peer1_addr = unwrap!(peer1.our_connection_info());

        let (mut peer2, ev_rx) = new_random_qp2p(false, Default::default());
        peer2.connect_to(peer1_addr);

        for event in ev_rx.iter() {
            if let Event::ConnectedTo { .. } = event {
                break;
            }
        }
        let (tx, rx) = mpsc::channel();
        peer2.el.post(move || {
            let contacted = ctx(|c| unwrap!(c.connections.get(&peer1_addr)).we_contacted_peer);
            let _ = tx.send(contacted);
        });
        let we_contacted_peer = unwrap!(rx.recv());

        assert!(we_contacted_peer);
    }

    #[test]
    fn is_hard_coded_contact() {
        let contact0 = rand_node_addr();
        let contact1 = rand_node_addr();

        let (peer, _) = new_random_qp2p(false, iter::once(contact0).collect());

        assert!(peer.is_hard_coded_contact(&contact0));
        assert!(!peer.is_hard_coded_contact(&contact1));
    }
}
