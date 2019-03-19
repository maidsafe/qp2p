#[macro_use]
extern crate quick_error;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate unwrap;

pub use config::{Config, SerialisableCeritificate};
pub use error::Error;
pub use event::Event;

use crate::wire_msg::WireMsg;
use context::{ctx, ctx_mut, initialise_ctx, Context};
use event_loop::EventLoop;
use std::net::SocketAddr;
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
mod wire_msg;

pub type R<T> = Result<T, Error>;

/// Main Crust instance to communicate with Crust
pub struct Crust {
    event_tx: Sender<Event>,
    cfg: Config,
    our_info: Option<CrustInfo>,
    el: EventLoop,
}

/// Crust information for peers to connect to each other
#[derive(Debug, Serialize, Deserialize, Clone)]
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
        let tx = self.event_tx.clone();

        let ((key, cert), our_complete_cert) = {
            let our_complete_cert = self
                .cfg
                .our_complete_cert
                .clone()
                .unwrap_or_else(Default::default);
            (
                our_complete_cert.obtain_priv_key_and_cert(),
                our_complete_cert
            )
        };

        self.el.post(move || {
            let server_cfg = Default::default();
            let mut server_cfg_builder = quinn::ServerConfigBuilder::new(server_cfg);
            unwrap!(server_cfg_builder
                .certificate(quinn::CertificateChain::from_certs(vec![cert]), key));

            let mut ep_builder = quinn::Endpoint::new();
            ep_builder.listen(server_cfg_builder.build());
            let (ep, dr, incoming_connections) =
                unwrap!(ep_builder.bind(&format!("127.0.0.1:{}", port)));

            let ctx = Context::new(tx, our_complete_cert, ep);
            initialise_ctx(ctx);

            current_thread::spawn(dr.map_err(|e| println!("Error in quinn Driver: {:?}", e)));
            listener::listen(incoming_connections);
        });
    }

    /// Connect to the given peer. This will error out if the peer is already in the process of
    /// being connected to OR for any other connection failure reasons.
    pub fn connect_to(&mut self, peer_info: CrustInfo) {
        self.el.post(move || {
            let _r = connect::connect_to(peer_info, None);
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
    pub fn our_connection_info(&mut self) -> R<CrustInfo> {
        if let Some(ref our_info) = self.our_info {
            return Ok(our_info.clone());
        }

        let our_ext_addr = self.query_ip_echo_service()?;
        let our_cert_der = self.our_certificate_der();

        let our_info = CrustInfo {
            peer_addr: our_ext_addr,
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
        let (tx, rx) = mpsc::channel();
        // FIXME: For the purpose of simplicity we are asking only one peer just now. In production
        // ask multiple until one answers OR we exhaust the list
        let echo_server_info = if let Some(peer_info) = self.cfg.hard_coded_contacts.first() {
            peer_info.clone()
        } else {
            return Err(Error::NoEndpointEchoServerFound);
        };

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
    use crate::config::{Config, SerialisableCeritificate};
    use std::net::SocketAddr;
    use std::str::FromStr;
    use std::sync::mpsc;

    #[test]
    fn dropping_crust_handle_gracefully_shutsdown_event_loop() {
        let (tx, _rx) = mpsc::channel();
        let mut crust = Crust::new(tx);
        crust.start_listening();
    }

    #[test]
    fn connect_to_peer_and_make_ep_echo_service_and_send_msgs() {
        // TODO make these ports random and obtain them
        const CRUST0_PORT: u16 = 55003;
        const CRUST1_PORT: u16 = 56003;

        let (tx0, rx0) = mpsc::channel();
        let (mut crust0, crust0_cert_der) = {
            let our_complete_cert = SerialisableCeritificate::default();
            let cert_der = our_complete_cert.cert_der.clone();

            let mut cfg: Config = Default::default();
            cfg.our_complete_cert = Some(our_complete_cert);
            cfg.port = Some(CRUST0_PORT);

            (Crust::with_config(tx0, cfg), cert_der)
        };

        let crust0_info = CrustInfo {
            peer_addr: unwrap!(SocketAddr::from_str(&format!("127.0.0.1:{}", CRUST0_PORT))),
            peer_cert_der: crust0_cert_der,
        };
        let crust0_addr = crust0_info.peer_addr;

        let (tx1, rx1) = mpsc::channel();
        let mut crust1 = {
            let mut cfg: Config = Default::default();
            cfg.hard_coded_contacts.push(crust0_info.clone());
            cfg.port = Some(CRUST1_PORT);
            Crust::with_config(tx1, cfg)
        };

        let crust1_addr = unwrap!(SocketAddr::from_str(&format!("127.0.0.1:{}", CRUST1_PORT)));

        crust0.start_listening();
        crust1.start_listening();

        // 50 MiB message
        let big_msg_to_crust0 = vec![255; 50]; // * 1024]; // * 1024];
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
                    Ok(Event::ConnectedTo { peer_addr }) => assert_eq!(peer_addr, crust1_addr),
                    Ok(x) => panic!("Expected Event::ConnectedTo - got {:?}", x),
                    Err(e) => panic!(
                        "Crust0 Expected Event::ConnectedTo; got error: {:?} {}",
                        e, e
                    ),
                };
                for i in 0..1 {
                    match rx0.recv() {
                        Ok(Event::NewMessage { peer_addr, msg }) => {
                            assert_eq!(peer_addr, crust1_addr);
                            // if i != 3 {
                            if i != 0 {
                                assert!(
                                    msg == small_msg0_to_crust0_clone
                                        || msg == small_msg1_to_crust0_clone
                                );
                            } else {
                                assert_eq!(msg, big_msg_to_crust0_clone);
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
                    Ok(Event::ConnectedTo { peer_addr }) => assert_eq!(peer_addr, crust0_addr),
                    Ok(x) => panic!("Expected Event::ConnectedTo - got {:?}", x),
                    Err(e) => panic!(
                        "Crust0 Expected Event::ConnectedTo; got error: {:?} {}",
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
                        "Crust0 Expected Event::NewMessage; got error: {:?} {}",
                        e, e
                    ),
                };
            }));

        crust1.connect_to(crust0_info.clone());

        let crust1_info = unwrap!(crust1.our_connection_info());
        assert_eq!(CRUST1_PORT, crust1_info.peer_addr.port());

        crust1.send(crust0_info.clone(), big_msg_to_crust0);
        //crust1.send(crust0_info.clone(), small_msg0_to_crust0);
        //crust1.send(crust0_info, small_msg1_to_crust0);
        crust0.send(crust1_info.clone(), msg_to_crust1);

        unwrap!(j0.join());
        unwrap!(j1.join());
    }
}
