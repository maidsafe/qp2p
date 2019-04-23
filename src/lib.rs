// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

// Required for the quick_error! macro
#![recursion_limit = "128"]

#[macro_use]
extern crate log;
#[macro_use]
extern crate quick_error;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate structopt;
#[macro_use]
extern crate unwrap;

pub use config::{Config, OurType, SerialisableCertificate};
pub use error::Error;
pub use event::Event;
pub use peer::{NodeInfo, Peer};
pub use peer_config::{DEFAULT_IDLE_TIMEOUT_MSEC, DEFAULT_KEEP_ALIVE_INTERVAL_MSEC};
pub use utils::R;

use crate::wire_msg::WireMsg;
use bootstrap_cache::BootstrapCache;
use context::{ctx, ctx_mut, initialise_ctx, Context};
use event_loop::EventLoop;
use std::collections::VecDeque;
use std::mem;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::mpsc::{self, Sender};
use tokio::prelude::Future;
use tokio::runtime::current_thread;

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
mod utils;
mod wire_msg;

/// Default maximum allowed message size. We'll error out on any bigger messages and probably
/// shutdown the connection. This value can be overridden via the `Config` option.
pub const DEFAULT_MAX_ALLOWED_MSG_SIZE: usize = 500 * 1024 * 1024; // 500MiB
/// In the absence of a port supplied by the user via the config we will first try using this
/// before using a random port.
pub const DEFAULT_PORT_TO_TRY: u16 = 443;

/// Builder for `QuicP2p`. Convenient for setting various parameters and creating `QuicP2p`.
pub struct Builder {
    event_tx: Sender<Event>,
    cfg: Option<Config>,
    proxies: VecDeque<NodeInfo>,
    use_proxies_exclusively: bool,
}

impl Builder {
    /// New `Builder`
    pub fn new(event_tx: Sender<Event>) -> Self {
        Self {
            event_tx,
            cfg: Default::default(),
            proxies: Default::default(),
            use_proxies_exclusively: Default::default(),
        }
    }

    /// Take proxies from the user.
    ///
    /// Either use these exclusively or in addition to the ones read from bootstrap cache file if
    /// such a file exists
    pub fn with_proxies(mut self, proxies: VecDeque<NodeInfo>, use_exclusively: bool) -> Self {
        self.use_proxies_exclusively = use_exclusively;

        if use_exclusively {
            self.proxies = proxies;
        } else {
            self.proxies.extend(proxies.into_iter());
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

        let use_proxies_exclusively = self.use_proxies_exclusively;
        let proxies = self.proxies;

        qp2p.el.post(move || {
            ctx_mut(|c| {
                if use_proxies_exclusively {
                    let _ = mem::replace(c.bootstrap_cache.peers_mut(), proxies);
                } else {
                    c.bootstrap_cache.peers_mut().extend(proxies.into_iter());
                }
            })
        });

        Ok(qp2p)
    }
}

/// Main QuicP2p instance to communicate with QuicP2p
pub struct QuicP2p {
    event_tx: Sender<Event>,
    cfg: Config,
    us: Option<NodeInfo>,
    el: EventLoop,
}

impl QuicP2p {
    /// Bootstrap to a proxy
    pub fn bootstrap(&mut self) {
        self.el.post(|| {
            bootstrap::start();
        })
    }

    /// Connect to the given peer. This will error out if the peer is already in the process of
    /// being connected to OR for any other connection failure reasons.
    pub fn connect_to(&mut self, peer_info: NodeInfo) {
        self.el.post(move || {
            let peer_addr = peer_info.peer_addr;
            if let Err(e) = connect::connect_to(peer_info, None, None) {
                info!(
                    "(TODO return this) Could not connect to the asked peer: {}",
                    e
                );
            } else {
                Self::set_we_contacted_peer(&peer_addr);
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
    pub fn send(&mut self, peer: Peer, msg: bytes::Bytes) {
        self.el.post(move || {
            let peer_addr = peer.peer_addr();
            communicate::try_write_to_peer(peer, WireMsg::UserMsg(msg));
            Self::set_we_contacted_peer(&peer_addr);
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
    pub fn our_connection_info(&mut self) -> R<NodeInfo> {
        if let Some(ref us) = self.us {
            return Ok(us.clone());
        }

        let our_addr = match self.query_ip_echo_service() {
            Ok(addr) => addr,
            Err(e @ Error::NoEndpointEchoServerFound) => {
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

        let our_cert_der = self.our_certificate_der();

        let us = NodeInfo {
            peer_addr: our_addr,
            peer_cert_der: our_cert_der,
        };

        self.us = Some(us.clone());

        Ok(us)
    }

    /// Retrieves current node bootstrap cache.
    pub fn bootstrap_cache(&mut self) -> R<Vec<NodeInfo>> {
        let (tx, rx) = mpsc::channel();
        self.el.post(move || {
            let cache = ctx(|c| c.bootstrap_cache.peers().iter().cloned().collect());
            let _ = tx.send(cache);
        });
        let cache = rx.recv()?;

        Ok(cache)
    }

    fn new(event_tx: Sender<Event>) -> R<Self> {
        Ok(Self::with_config(
            event_tx,
            Config::read_or_construct_default(None)?,
        ))
    }

    fn with_config(event_tx: Sender<Event>, cfg: Config) -> Self {
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
            .unwrap_or(peer_config::DEFAULT_IDLE_TIMEOUT_MSEC);
        let keep_alive_interval_msec = self
            .cfg
            .keep_alive_interval_msec
            .unwrap_or(peer_config::DEFAULT_KEEP_ALIVE_INTERVAL_MSEC);
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
                our_complete_cert.obtain_priv_key_and_cert(),
                our_complete_cert,
            )
        };
        let bootstrap_cache = BootstrapCache::new(hard_coded_contacts, None)?;

        self.el.post(move || {
            let our_cfg = unwrap!(peer_config::new_our_cfg(
                idle_timeout_msec,
                keep_alive_interval_msec,
                cert,
                key
            ));

            let mut ep_builder = quinn::Endpoint::builder();
            ep_builder.listen(our_cfg);
            let (dr, ep, incoming_connections) = {
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
                        unwrap!(ep_builder.bind(&(ip, 0)))
                    }
                }
            };

            let ctx = Context::new(
                tx,
                our_complete_cert,
                max_msg_size_allowed,
                idle_timeout_msec,
                keep_alive_interval_msec,
                our_type,
                bootstrap_cache,
                ep,
            );
            initialise_ctx(ctx);

            current_thread::spawn(dr.map_err(|e| warn!("Error in quinn Driver: {:?}", e)));

            if our_type != OurType::Client {
                listener::listen(incoming_connections);
            }
        });

        Ok(())
    }

    fn our_certificate_der(&mut self) -> Vec<u8> {
        let (tx, rx) = mpsc::channel();

        self.el.post(move || {
            let our_cert_der = ctx(|c| c.our_complete_cert.cert_der.clone());
            unwrap!(tx.send(our_cert_der));
        });

        unwrap!(rx.recv())
    }

    fn query_ip_echo_service(&mut self) -> R<SocketAddr> {
        // FIXME: For the purpose of simplicity we are asking only one peer just now. In production
        // ask multiple until one answers OR we exhaust the list
        let node_info = if let Some(node_info) = self.cfg.hard_coded_contacts.iter().next() {
            node_info.clone()
        } else {
            return Err(Error::NoEndpointEchoServerFound);
        };
        let echo_server = Peer::Node { node_info };

        let (tx, rx) = mpsc::channel();

        self.el.post(move || {
            ctx_mut(|c| c.our_ext_addr_tx = Some(tx));
            communicate::try_write_to_peer(echo_server, WireMsg::EndpointEchoReq)
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
    use std::collections::HashSet;
    use std::sync::mpsc::{self, Receiver};

    #[test]
    fn dropping_qp2p_handle_gracefully_shutsdown_event_loop() {
        let (tx, _rx) = mpsc::channel();
        let _qp2p = unwrap!(Builder::new(tx).build());
    }

    #[test]
    fn echo_service() {
        let (mut qp2p0, _rx) = new_random_qp2p_for_unit_test(false, Default::default());

        // Confirm there's no echo service available for us
        match qp2p0.query_ip_echo_service() {
            Ok(_) => panic!("Without Hard Coded Contacts, echo service should not be possible"),
            Err(Error::NoEndpointEchoServerFound) => (),
            Err(e) => panic!("{:?} - {}", e, e),
        }

        // Now the only way to obtain info is via querring the quic_ep for the bound address
        let qp2p0_info = unwrap!(qp2p0.our_connection_info());
        let qp2p0_port = qp2p0_info.peer_addr.port();

        let (mut qp2p1, rx1) = {
            let mut hcc: HashSet<_> = Default::default();
            assert!(hcc.insert(qp2p0_info.clone()));
            new_random_qp2p_for_unit_test(true, hcc)
        };

        // Echo service is availabe through qp2p0
        let qp2p1_port = unwrap!(qp2p1.query_ip_echo_service()).port();
        let qp2p1_info = unwrap!(qp2p1.our_connection_info());

        assert_ne!(qp2p0_port, qp2p1_port);
        assert_eq!(qp2p1_port, qp2p1_info.peer_addr.port());

        let (mut qp2p2, _rx) = {
            let mut hcc: HashSet<_> = Default::default();
            assert!(hcc.insert(qp2p0_info.clone()));
            new_random_qp2p_for_unit_test(true, hcc)
        };
        let qp2p2_info = unwrap!(qp2p2.our_connection_info());

        // The two qp2p can now send data to each other
        // Drain the receiver first
        while let Ok(_) = rx1.try_recv() {}

        let data = bytes::Bytes::from(vec![12, 13, 14, 253]);
        qp2p2.send(qp2p1_info.into(), data.clone());

        match unwrap!(rx1.recv()) {
            Event::ConnectedTo { peer } => assert_eq!(
                peer,
                Peer::Node {
                    node_info: qp2p2_info.clone()
                }
            ),
            x => panic!("Received unexpected event: {:?}", x),
        }
        match unwrap!(rx1.recv()) {
            Event::NewMessage { peer_addr, msg } => {
                assert_eq!(peer_addr, qp2p2_info.peer_addr);
                assert_eq!(msg, data);
            }
            x => panic!("Received unexpected event: {:?}", x),
        }
    }

    #[test]
    fn multistreaming_and_no_head_of_queue_blocking() {
        let (mut qp2p0, rx0) = new_random_qp2p_for_unit_test(false, Default::default());
        let qp2p0_info = unwrap!(qp2p0.our_connection_info());

        let (mut qp2p1, rx1) = {
            let mut hcc: HashSet<_> = Default::default();
            assert!(hcc.insert(qp2p0_info.clone()));
            new_random_qp2p_for_unit_test(true, hcc)
        };
        let qp2p1_info = unwrap!(qp2p1.our_connection_info());

        let qp2p0_addr = qp2p0_info.peer_addr;
        let qp2p1_addr = qp2p1_info.peer_addr;

        // 400 MiB message
        let big_msg_to_qp2p0 = bytes::Bytes::from(vec![255; 400 * 1024 * 1024]);
        let big_msg_to_qp2p0_clone = big_msg_to_qp2p0.clone();

        // very small messages
        let small_msg0_to_qp2p0 = bytes::Bytes::from(vec![255, 254, 253, 252]);
        let small_msg0_to_qp2p0_clone = small_msg0_to_qp2p0.clone();

        let small_msg1_to_qp2p0 = bytes::Bytes::from(vec![155, 154, 153, 152]);
        let small_msg1_to_qp2p0_clone = small_msg1_to_qp2p0.clone();

        let msg_to_qp2p1 = bytes::Bytes::from(vec![120, 129, 2]);
        let msg_to_qp2p1_clone = msg_to_qp2p1.clone();

        let j0 = unwrap!(std::thread::Builder::new()
            .name("QuicP2p0-test-thread".to_string())
            .spawn(move || {
                match rx0.recv() {
                    Ok(Event::ConnectedTo {
                        peer: Peer::Node { node_info },
                    }) => assert_eq!(node_info.peer_addr, qp2p1_addr),
                    Ok(x) => panic!("Expected Event::ConnectedTo - got {:?}", x),
                    Err(e) => panic!(
                        "QuicP2p0 Expected Event::ConnectedTo; got error: {:?} {}",
                        e, e
                    ),
                };
                for i in 0..3 {
                    match rx0.recv() {
                        Ok(Event::NewMessage { peer_addr, msg }) => {
                            assert_eq!(peer_addr, qp2p1_addr);
                            if i != 2 {
                                assert!(
                                    msg == small_msg0_to_qp2p0_clone
                                        || msg == small_msg1_to_qp2p0_clone
                                );
                                info!("Smaller message {:?} rxd from {}", &*msg, peer_addr)
                            } else {
                                assert_eq!(msg, big_msg_to_qp2p0_clone);
                                info!("Big message of size {} rxd from {}", msg.len(), peer_addr);
                            }
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
                        peer: Peer::Node { node_info },
                    }) => assert_eq!(node_info.peer_addr, qp2p0_addr),
                    Ok(x) => panic!("Expected Event::ConnectedTo - got {:?}", x),
                    Err(e) => panic!(
                        "QuicP2p1 Expected Event::ConnectedTo; got error: {:?} {}",
                        e, e
                    ),
                };
                match rx1.recv() {
                    Ok(Event::NewMessage { peer_addr, msg }) => {
                        assert_eq!(peer_addr, qp2p0_addr);
                        assert_eq!(msg, msg_to_qp2p1_clone);
                    }
                    Ok(x) => panic!("Expected Event::NewMessage - got {:?}", x),
                    Err(e) => panic!(
                        "QuicP2p1 Expected Event::NewMessage; got error: {:?} {}",
                        e, e
                    ),
                };
            }));

        // Send the biggest message first and we'll assert that it arrives last hence not blocking
        // the rest of smaller messages sent after it
        qp2p1.send(qp2p0_info.clone().into(), big_msg_to_qp2p0);
        qp2p1.send(qp2p0_info.clone().into(), small_msg0_to_qp2p0);
        // Even after a delay the following small message should arrive before the 1st sent big
        // message
        std::thread::sleep(std::time::Duration::from_millis(100));
        qp2p1.send(qp2p0_info.into(), small_msg1_to_qp2p0);

        qp2p0.send(qp2p1_info.into(), msg_to_qp2p1);

        unwrap!(j0.join());
        unwrap!(j1.join());
    }

    #[test]
    fn connect_to_marks_that_we_attempted_to_contact_the_peer() {
        let (mut peer1, _) = new_random_qp2p_for_unit_test(false, Default::default());
        let peer1_conn_info = unwrap!(peer1.our_connection_info());
        let peer1_addr = peer1_conn_info.peer_addr;

        let (mut peer2, ev_rx) = new_random_qp2p_for_unit_test(false, Default::default());
        peer2.connect_to(peer1_conn_info);

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

    fn new_random_qp2p_for_unit_test(
        is_addr_unspecified: bool,
        contacts: HashSet<NodeInfo>,
    ) -> (QuicP2p, Receiver<Event>) {
        let (tx, rx) = mpsc::channel();
        let qp2p = {
            let mut cfg = Config::with_default_cert();
            cfg.hard_coded_contacts = contacts;
            cfg.port = Some(0);
            if !is_addr_unspecified {
                cfg.ip = Some(IpAddr::V4(Ipv4Addr::LOCALHOST));
            }
            unwrap!(Builder::new(tx).with_config(cfg).build())
        };

        (qp2p, rx)
    }
}
