#[macro_use]
extern crate quick_error;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate unwrap;

pub use config::{Config, SerialisableCertificate};
pub use error::Error;
pub use event::Event;
pub use peer_config::{DEFAULT_IDLE_TIMEOUT_MSEC, DEFAULT_KEEP_ALIVE_INTERVAL_MSEC};

use crate::wire_msg::WireMsg;
use context::{ctx, ctx_mut, initialise_ctx, Context};
use event_loop::EventLoop;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::mpsc::{self, Sender};
use tokio::prelude::Future;
use tokio::runtime::current_thread;

mod communicate;
mod config;
mod connect;
mod context;
mod error;
mod event;
mod event_loop;
mod listener;
mod peer_config;
mod wire_msg;

pub type R<T> = Result<T, Error>;

/// Default maximum allowed message size. We'll error out on any bigger messages and probably
/// shutdown the connection. This value can be overridden via the `Config` option.
pub const DEFAULT_MAX_ALLOWED_MSG_SIZE: usize = 500 * 1024 * 1024; // 500MiB

/// Main Crust instance to communicate with Crust
pub struct Crust {
    event_tx: Sender<Event>,
    cfg: Config,
    our_info: Option<CrustInfo>,
    el: EventLoop,
}

/// Crust information for peers to connect to each other
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub struct CrustInfo {
    pub peer_addr: SocketAddr,
    pub peer_cert_der: Vec<u8>,
}

impl Crust {
    /// Create a new Crust instance
    pub fn new(event_tx: Sender<Event>) -> Self {
        Self::with_config(event_tx, Config::read_or_construct_default())
    }

    /// Create a new Crust instance with supplied Configuration
    pub fn with_config(event_tx: Sender<Event>, cfg: Config) -> Self {
        let el = EventLoop::spawn();
        Self {
            event_tx,
            cfg,
            our_info: None,
            el,
        }
    }

    /// Start listener
    ///
    /// It is necessary to call this to initialise Crust context within the event loop. Otherwise
    /// very limited functionaity will be available.
    pub fn start_listening(&mut self) {
        let port = self.cfg.port.unwrap_or(0);
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

        self.el.post(move || {
            let our_cfg = unwrap!(peer_config::new_our_cfg(
                idle_timeout_msec,
                keep_alive_interval_msec,
                cert,
                key
            ));

            let mut ep_builder = quinn::Endpoint::new();
            ep_builder.listen(our_cfg);
            let (dr, ep, incoming_connections) = unwrap!(ep_builder.bind(&(ip, port)));

            let ctx = Context::new(
                tx,
                our_complete_cert,
                max_msg_size_allowed,
                idle_timeout_msec,
                keep_alive_interval_msec,
                ep,
            );
            initialise_ctx(ctx);

            current_thread::spawn(dr.map_err(|e| println!("Error in quinn Driver: {:?}", e)));
            listener::listen(incoming_connections);
        });
    }

    /// Connect to the given peer. This will error out if the peer is already in the process of
    /// being connected to OR for any other connection failure reasons.
    pub fn connect_to(&mut self, peer_info: CrustInfo) {
        self.el.post(move || {
            if let Err(e) = connect::connect_to(peer_info, None) {
                println!(
                    "(TODO return this) Could not connect to the asked peer: {}",
                    e
                );
            }
        });
    }

    /// Disconnect from the given peer
    pub fn disconnect_from(&mut self, peer_addr: SocketAddr) {
        self.el.post(move || {
            ctx_mut(|c| {
                // TODO test if closing quinn::Connection destroys all `write_all` streams. If
                // there's bug upstream then our writer might not be resolved until a long time
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
    pub fn send(&mut self, peer_info: CrustInfo, msg: Vec<u8>) {
        self.el
            .post(move || communicate::try_write_to_peer(peer_info, WireMsg::UserMsg(msg)));
    }

    /// Get our connection info to give to others for them to connect to us
    ///
    /// Will use hard coded contacts to ask for our endpoint. If no contact is given then we'll
    /// simply build our connection info by querying the underlying bound socket for our address.
    /// Note that if such an obtained address is of unspecified category we will ignore that as
    /// such an address cannot be reached and hence not useful.
    // FIXME calling this mutliple times concurrently just now could have it hanging as only one tx
    // is registered and that replaces any previous tx registered. Fix by using a vec of txs
    pub fn our_connection_info(&mut self) -> R<CrustInfo> {
        if let Some(ref our_info) = self.our_info {
            return Ok(our_info.clone());
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

        let our_info = CrustInfo {
            peer_addr: our_addr,
            peer_cert_der: our_cert_der,
        };

        self.our_info = Some(our_info.clone());

        Ok(our_info)
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
        let echo_server_info = if let Some(peer_info) = self.cfg.hard_coded_contacts.first() {
            peer_info.clone()
        } else {
            return Err(Error::NoEndpointEchoServerFound);
        };

        let (tx, rx) = mpsc::channel();

        self.el.post(move || {
            ctx_mut(|c| c.our_ext_addr_tx = Some(tx));
            communicate::try_write_to_peer(echo_server_info, WireMsg::EndpointEchoReq)
        });

        Ok(unwrap!(rx.recv()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{self, Receiver};

    #[test]
    fn dropping_crust_handle_gracefully_shutsdown_event_loop() {
        let (tx, _rx) = mpsc::channel();
        let mut crust = Crust::new(tx);
        crust.start_listening();
    }

    #[test]
    fn echo_service() {
        let (mut crust0, _rx) = new_random_crust_for_unit_test(false, Default::default());

        // Confirm there's no echo service available for us
        match crust0.query_ip_echo_service() {
            Ok(_) => panic!("Without Hard Coded Contacts, echo service should not be possible"),
            Err(Error::NoEndpointEchoServerFound) => (),
            Err(e) => panic!("{:?} - {}", e, e),
        }

        // Now the only way to obtain info is via querring the quic_ep for the bound address
        let crust0_info = unwrap!(crust0.our_connection_info());
        let crust0_port = crust0_info.peer_addr.port();

        let (mut crust1, rx1) = new_random_crust_for_unit_test(true, vec![crust0_info.clone()]);

        // Echo service is availabe through crust0
        let crust1_port = unwrap!(crust1.query_ip_echo_service()).port();
        let crust1_info = unwrap!(crust1.our_connection_info());

        assert_ne!(crust0_port, crust1_port);
        assert_eq!(crust1_port, crust1_info.peer_addr.port());

        let (mut crust2, _rx) = new_random_crust_for_unit_test(true, vec![crust0_info.clone()]);
        let crust2_info = unwrap!(crust2.our_connection_info());

        // The two crust can now send data to each other
        // Drain the receiver first
        while let Ok(_) = rx1.try_recv() {}

        let data = vec![12, 13, 14, 253];
        crust2.send(crust1_info, data.clone());

        match unwrap!(rx1.recv()) {
            Event::ConnectedTo { crust_info } => assert_eq!(crust_info, crust2_info),
            x => panic!("Received unexpected event: {:?}", x),
        }
        match unwrap!(rx1.recv()) {
            Event::NewMessage { peer_addr, msg } => {
                assert_eq!(peer_addr, crust2_info.peer_addr);
                assert_eq!(msg, data);
            }
            x => panic!("Received unexpected event: {:?}", x),
        }
    }

    #[test]
    fn multistreaming_and_no_head_of_queue_blocking() {
        let (mut crust0, rx0) = new_random_crust_for_unit_test(false, Default::default());
        let crust0_info = unwrap!(crust0.our_connection_info());

        let (mut crust1, rx1) = new_random_crust_for_unit_test(true, vec![crust0_info.clone()]);
        let crust1_info = unwrap!(crust1.our_connection_info());

        let crust0_addr = crust0_info.peer_addr;
        let crust1_addr = crust1_info.peer_addr;

        // 400 MiB message
        let big_msg_to_crust0 = vec![255; 400 * 1024 * 1024];
        let big_msg_to_crust0_clone = big_msg_to_crust0.clone();

        // very small messages
        let small_msg0_to_crust0 = vec![255, 254, 253, 252];
        let small_msg0_to_crust0_clone = small_msg0_to_crust0.clone();

        let small_msg1_to_crust0 = vec![155, 154, 153, 152];
        let small_msg1_to_crust0_clone = small_msg1_to_crust0.clone();

        let msg_to_crust1 = vec![120, 129, 2];
        let msg_to_crust1_clone = msg_to_crust1.clone();

        let j0 = unwrap!(std::thread::Builder::new()
            .name("Crust0-test-thread".to_string())
            .spawn(move || {
                match rx0.recv() {
                    Ok(Event::ConnectedTo { crust_info }) => {
                        assert_eq!(crust_info.peer_addr, crust1_addr)
                    }
                    Ok(x) => panic!("Expected Event::ConnectedTo - got {:?}", x),
                    Err(e) => panic!(
                        "Crust0 Expected Event::ConnectedTo; got error: {:?} {}",
                        e, e
                    ),
                };
                for i in 0..3 {
                    match rx0.recv() {
                        Ok(Event::NewMessage { peer_addr, msg }) => {
                            assert_eq!(peer_addr, crust1_addr);
                            if i != 2 {
                                assert!(
                                    msg == small_msg0_to_crust0_clone
                                        || msg == small_msg1_to_crust0_clone
                                );
                                println!("Smaller message {:?} rxd from {}", msg, peer_addr)
                            } else {
                                assert_eq!(msg, big_msg_to_crust0_clone);
                                println!(
                                    "Big message of size {} rxd from {}",
                                    msg.len(),
                                    peer_addr
                                );
                            }
                        }
                        Ok(x) => panic!("Expected Event::NewMessage - got {:?}", x),
                        Err(e) => panic!(
                            "Crust0 Expected Event::NewMessage; got error: {:?} {}",
                            e, e
                        ),
                    };
                }
            }));
        let j1 = unwrap!(std::thread::Builder::new()
            .name("Crust1-test-thread".to_string())
            .spawn(move || {
                match rx1.recv() {
                    Ok(Event::ConnectedTo { crust_info }) => {
                        assert_eq!(crust_info.peer_addr, crust0_addr)
                    }
                    Ok(x) => panic!("Expected Event::ConnectedTo - got {:?}", x),
                    Err(e) => panic!(
                        "Crust1 Expected Event::ConnectedTo; got error: {:?} {}",
                        e, e
                    ),
                };
                match rx1.recv() {
                    Ok(Event::NewMessage { peer_addr, msg }) => {
                        assert_eq!(peer_addr, crust0_addr);
                        assert_eq!(msg, msg_to_crust1_clone);
                    }
                    Ok(x) => panic!("Expected Event::NewMessage - got {:?}", x),
                    Err(e) => panic!(
                        "Crust1 Expected Event::NewMessage; got error: {:?} {}",
                        e, e
                    ),
                };
            }));

        // Send the biggest message first and we'll assert that it arrives last hence not blocking
        // the rest of smaller messages sent after it
        crust1.send(crust0_info.clone(), big_msg_to_crust0);
        crust1.send(crust0_info.clone(), small_msg0_to_crust0);
        // Even after a delay the following small message should arrive before the 1st sent big
        // message
        std::thread::sleep(std::time::Duration::from_millis(100));
        crust1.send(crust0_info, small_msg1_to_crust0);

        crust0.send(crust1_info, msg_to_crust1);

        unwrap!(j0.join());
        unwrap!(j1.join());
    }

    fn new_random_crust_for_unit_test(
        is_addr_unspecified: bool,
        contacts: Vec<CrustInfo>,
    ) -> (Crust, Receiver<Event>) {
        let (tx, rx) = mpsc::channel();
        let mut crust = {
            let mut cfg = Config::with_default_cert();
            cfg.hard_coded_contacts = contacts;
            if !is_addr_unspecified {
                cfg.ip = Some(IpAddr::V4(Ipv4Addr::LOCALHOST));
            }
            Crust::with_config(tx, cfg)
        };

        crust.start_listening();

        (crust, rx)
    }
}
