// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

#[macro_use]
extern crate quick_error;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate unwrap;

pub use config::{Config, OurType, SerialisableCertificate};
pub use error::Error;
pub use event::Event;
pub use peer_config::{DEFAULT_IDLE_TIMEOUT_MSEC, DEFAULT_KEEP_ALIVE_INTERVAL_MSEC};

use crate::bootstrap_cache::init_bootstrap_cache;
use crate::wire_msg::WireMsg;
use context::{ctx, ctx_mut, initialise_ctx, Context};
use event_loop::EventLoop;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::mpsc::{self, Sender};
use tokio::prelude::Future;
use tokio::runtime::current_thread;

mod bootstrap_cache;
mod communicate;
mod config;
mod connect;
mod context;
mod dirs;
mod error;
mod event;
mod event_loop;
mod listener;
mod peer_config;
mod utils;
mod wire_msg;

pub type R<T> = Result<T, Error>;

/// Default maximum allowed message size. We'll error out on any bigger messages and probably
/// shutdown the connection. This value can be overridden via the `Config` option.
pub const DEFAULT_MAX_ALLOWED_MSG_SIZE: usize = 500 * 1024 * 1024; // 500MiB
/// In the absence of a port supplied by the user via the config we will first try using this
/// before using a random port.
pub const DEFAULT_PORT_TO_TRY: u16 = 443;

/// Main QuicP2p instance to communicate with QuicP2p
pub struct QuicP2p {
    event_tx: Sender<Event>,
    cfg: Config,
    us: Option<NodeInfo>,
    el: EventLoop,
}

/// A peer to us
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub enum Peer {
    Node { node_info: NodeInfo },
    Client { peer_addr: SocketAddr },
}

impl Peer {
    pub fn peer_addr(&self) -> SocketAddr {
        match *self {
            Peer::Node { ref node_info } => node_info.peer_addr,
            Peer::Client { peer_addr } => peer_addr,
        }
    }

    pub fn peer_cert_der(&self) -> Option<&[u8]> {
        match *self {
            Peer::Node { ref node_info } => Some(&node_info.peer_cert_der),
            Peer::Client { .. } => None,
        }
    }
}

/// Information for a peer of type `Peer::Node`.
///
/// This is a necessary information needed to connect to someone.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub struct NodeInfo {
    pub peer_addr: SocketAddr,
    pub peer_cert_der: Vec<u8>,
}

impl Into<Peer> for NodeInfo {
    fn into(self) -> Peer {
        Peer::Node { node_info: self }
    }
}

impl fmt::Display for NodeInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "NodeInfo {{ peer_addr: {}, peer_cert_der: {} }}",
            self.peer_addr,
            utils::bin_data_format(&self.peer_cert_der)
        )
    }
}

impl QuicP2p {
    /// Create a new QuicP2p instance
    pub fn new(event_tx: Sender<Event>) -> Self {
        Self::with_config(event_tx, Config::read_or_construct_default())
    }

    /// Create a new QuicP2p instance with supplied Configuration
    pub fn with_config(event_tx: Sender<Event>, cfg: Config) -> Self {
        let el = EventLoop::spawn();
        Self {
            event_tx,
            cfg,
            us: None,
            el,
        }
    }

    /// Start listener
    ///
    /// It is necessary to call this to initialise QuicP2p context within the event loop. Otherwise
    /// very limited functionaity will be available.
    pub fn start_listening(&mut self) -> R<()> {
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

        let (err_tx, err_rx) = mpsc::channel();
        self.el.post(move || {
            let our_cfg = unwrap!(peer_config::new_our_cfg(
                idle_timeout_msec,
                keep_alive_interval_msec,
                cert,
                key
            ));

            let mut ep_builder = quinn::Endpoint::new();
            ep_builder.listen(our_cfg);
            let (dr, ep, incoming_connections) = {
                match UdpSocket::bind(&(ip, port)) {
                    Ok(udp) => unwrap!(ep_builder.from_socket(udp)),
                    Err(e) => {
                        if is_user_supplied {
                            panic!(
                                "Could not bind to the user supplied port: {}! Error: {:?}- {}",
                                port, e, e
                            );
                        }
                        println!(
                            "Failed to bind to port: {} - Error: {:?} - {}. Trying random port.",
                            DEFAULT_PORT_TO_TRY, e, e
                        );
                        unwrap!(ep_builder.bind(&(ip, 0)))
                    }
                }
            };

            let bootstrap_cache = match init_bootstrap_cache(hard_coded_contacts) {
                Ok(cache) => cache,
                Err(e) => {
                    let _ = err_tx.send(Err(e));
                    return;
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

            current_thread::spawn(dr.map_err(|e| println!("Error in quinn Driver: {:?}", e)));

            if our_type != OurType::Client {
                listener::listen(incoming_connections);
            }

            let _ = err_tx.send(Ok(()));
        });

        err_rx
            .recv()
            .map_err(Error::ChannelRecv)
            .and_then(|res| res)
    }

    /// Connect to the given peer. This will error out if the peer is already in the process of
    /// being connected to OR for any other connection failure reasons.
    pub fn connect_to(&mut self, peer_info: NodeInfo) {
        self.el.post(move || {
            let peer_addr = peer_info.peer_addr;
            if let Err(e) = connect::connect_to(peer_info, None) {
                println!(
                    "(TODO return this) Could not connect to the asked peer: {}",
                    e
                );
            } else {
                flag_valid_connection(&peer_addr);
            }
        });
    }

    /// Disconnect from the given peer
    pub fn disconnect_from(&mut self, peer_addr: SocketAddr) {
        self.el.post(move || {
            ctx_mut(|c| {
                if c.connections.remove(&peer_addr).is_none() {
                    println!("Asked to disconnect from an unknown peer");
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
            flag_valid_connection(&peer_addr);
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
        let node_info = if let Some(node_info) = self.cfg.hard_coded_contacts.first() {
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
}

fn flag_valid_connection(peer_addr: &SocketAddr) {
    ctx_mut(|c| {
        if let Some(conn) = c.connections.get_mut(peer_addr) {
            conn.we_contacted_peer = true;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{self, Receiver};

    #[test]
    fn dropping_qp2p_handle_gracefully_shutsdown_event_loop() {
        let (tx, _rx) = mpsc::channel();
        let mut qp2p = QuicP2p::new(tx);
        unwrap!(qp2p.start_listening());
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

        let (mut qp2p1, rx1) = new_random_qp2p_for_unit_test(true, vec![qp2p0_info.clone()]);

        // Echo service is availabe through qp2p0
        let qp2p1_port = unwrap!(qp2p1.query_ip_echo_service()).port();
        let qp2p1_info = unwrap!(qp2p1.our_connection_info());

        assert_ne!(qp2p0_port, qp2p1_port);
        assert_eq!(qp2p1_port, qp2p1_info.peer_addr.port());

        let (mut qp2p2, _rx) = new_random_qp2p_for_unit_test(true, vec![qp2p0_info.clone()]);
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

        let (mut qp2p1, rx1) = new_random_qp2p_for_unit_test(true, vec![qp2p0_info.clone()]);
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
                                println!("Smaller message {:?} rxd from {}", &*msg, peer_addr)
                            } else {
                                assert_eq!(msg, big_msg_to_qp2p0_clone);
                                println!(
                                    "Big message of size {} rxd from {}",
                                    msg.len(),
                                    peer_addr
                                );
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

    fn new_random_qp2p_for_unit_test(
        is_addr_unspecified: bool,
        contacts: Vec<NodeInfo>,
    ) -> (QuicP2p, Receiver<Event>) {
        let (tx, rx) = mpsc::channel();
        let mut qp2p = {
            let mut cfg = Config::with_default_cert();
            cfg.hard_coded_contacts = contacts;
            cfg.port = Some(0);
            if !is_addr_unspecified {
                cfg.ip = Some(IpAddr::V4(Ipv4Addr::LOCALHOST));
            }
            QuicP2p::with_config(tx, cfg)
        };

        unwrap!(qp2p.start_listening());

        (qp2p, rx)
    }
}
