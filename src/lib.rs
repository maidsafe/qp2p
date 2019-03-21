#[macro_use]
extern crate quick_error;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate unwrap;

pub use config::{Config, SerialisableCertificate};
pub use error::Error;
pub use event::Event;

use crate::wire_msg::WireMsg;
use context::{ctx, ctx_mut, initialise_ctx, Context};
use event_loop::EventLoop;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
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

/// Default maximum allowed message size. We'll error out on any bigger messages and probably
/// shutdown the connection. This value can be overridden via the `Config` option.
pub const DEFAULT_MAX_ALLOWED_MSG_SIZE: usize = 500 * 1024 * 1024; // 500MiB
/// Default interval within which if we hear nothing from the peer we declare it offline to us.
///
/// This is based on average time in which routers would close the UDP mapping to the peer if they
/// see no conversation between them.
///
/// The value is in seconds.
pub const DEFAULT_IDLE_TIMEOUT: u64 = 30; // 30secs
/// Default Interval to send keep-alives if we are idling so that the peer does not disconnect from
/// us declaring us offline. If none is supplied we'll default to the documented constant.
///
/// The value is in seconds.
pub const DEFAULT_KEEP_ALIVE_INTERVAL: u32 = 10; // 10secs

/// Main Crust instance to communicate with Crust
pub struct Crust {
    event_tx: Sender<Event>,
    cfg: Config,
    our_global_info: Option<CrustInfo>,
    our_localhost_connection_info: Option<CrustInfo>,
    el: EventLoop,
}

/// Crust information for peers to connect to each other
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
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
            our_global_info: None,
            our_localhost_connection_info: None,
            el,
        }
    }

    /// Start listener
    ///
    /// It is necessary to call this to initialise Crust context within the event loop. Otherwise
    /// very limited functionaity will be available.
    pub fn start_listening(&mut self) {
        let port = self.cfg.port.unwrap_or(0);
        let ip = self.cfg.ip.unwrap_or_else(|| Ipv4Addr::new(0, 0, 0, 0));
        let max_msg_size_allowed = self
            .cfg
            .max_msg_size_allowed
            .map(|size| size as usize)
            .unwrap_or(DEFAULT_MAX_ALLOWED_MSG_SIZE);
        let idle_timeout = self.cfg.idle_timeout.unwrap_or(DEFAULT_IDLE_TIMEOUT);
        let keep_alive_interval = self
            .cfg
            .keep_alive_interval
            .unwrap_or(DEFAULT_KEEP_ALIVE_INTERVAL);

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
            let our_cfg = Default::default();
            let mut our_cfg_builder = quinn::ServerConfigBuilder::new(our_cfg);
            unwrap!(
                our_cfg_builder.certificate(quinn::CertificateChain::from_certs(vec![cert]), key)
            );
            let mut our_cfg = our_cfg_builder.build();
            {
                let transport_config = unwrap!(Arc::get_mut(&mut our_cfg.transport_config));
                // TODO test that this is sent only over the uni-stream to the peer not on the uni
                // stream from the peer
                transport_config.idle_timeout = idle_timeout;
                transport_config.keep_alive_interval = keep_alive_interval;
            }

            let mut ep_builder = quinn::Endpoint::new();
            ep_builder.listen(our_cfg);
            let (ep, dr, incoming_connections) = unwrap!(ep_builder.bind(&(ip, port)));

            let ctx = Context::new(
                tx,
                our_complete_cert,
                max_msg_size_allowed,
                idle_timeout,
                keep_alive_interval,
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
                println!("Could not connect to the asked peer: {}", e);
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
    // FIXME calling this mutliple times just now could have it hanging as only one tx is
    // registered and that replaces any previous tx registered. Fix by using a vec of txs
    pub fn our_global_connection_info(&mut self) -> R<CrustInfo> {
        if let Some(ref our_global_info) = self.our_global_info {
            return Ok(our_global_info.clone());
        }

        let our_ext_addr = self.query_ip_echo_service()?;
        let our_cert_der = self.our_certificate_der();

        let our_global_info = CrustInfo {
            peer_addr: our_ext_addr,
            peer_cert_der: our_cert_der,
        };

        self.our_global_info = Some(our_global_info.clone());

        Ok(our_global_info)
    }

    /// Get the `CrustInfo` with actual port we are locally bound to.
    ///
    /// This is our localhost info and useful for running in the context of the localhost, mainly
    /// for testing. The port should be the same as the given port in the configuration file or if
    /// none supplied there it will return the OS allocated random port to which we bound ourselves
    /// to.
    pub fn our_localhost_connection_info(&mut self) -> R<CrustInfo> {
        if let Some(ref our_localhost_connection_info) = self.our_localhost_connection_info {
            return Ok(our_localhost_connection_info.clone());
        }

        let (tx, rx) = mpsc::channel();

        self.el.post(move || {
            let our_local_addr_res = ctx(|c| c.quic_ep().local_addr());
            unwrap!(tx.send(our_local_addr_res));
        });

        let our_local_addr = unwrap!(rx.recv())?;
        let our_cert_der = self.our_certificate_der();

        let our_localhost_connection_info = CrustInfo {
            peer_addr: our_local_addr,
            peer_cert_der: our_cert_der,
        };

        self.our_localhost_connection_info = Some(our_localhost_connection_info.clone());

        Ok(our_localhost_connection_info)
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
        let echo_server_info = if let Some(peer_info) =
            self.cfg.hard_coded_contacts.iter().find(|contact| {
                let ip = contact.peer_addr.ip();

                cfg!(test)
                    || if let IpAddr::V4(v4) = contact.peer_addr.ip() {
                        !v4.is_private() && !v4.is_link_local() && !v4.is_loopback()
                    } else {
                        !ip.is_loopback()
                    }
            }) {
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
    use crate::config::{Config, SerialisableCertificate};
    use std::net::SocketAddr;
    use std::str::FromStr;
    use std::sync::mpsc;

    #[test]
    fn local_port() {
        let (tx0, _rx0) = mpsc::channel();
        let (mut crust0, crust0_cert_der) = {
            let our_complete_cert = SerialisableCertificate::default();
            let cert_der = our_complete_cert.cert_der.clone();

            let mut cfg: Config = Default::default();
            cfg.our_complete_cert = Some(our_complete_cert);

            (Crust::with_config(tx0, cfg), cert_der)
        };
        crust0.start_listening();

        let crust0_port = unwrap!(crust0.our_localhost_connection_info())
            .peer_addr
            .port();

        let crust0_info = CrustInfo {
            peer_addr: unwrap!(SocketAddr::from_str(&format!("127.0.0.1:{}", crust0_port))),
            peer_cert_der: crust0_cert_der,
        };

        let (tx1, _rx1) = mpsc::channel();
        let mut crust1 = {
            let mut cfg: Config = Default::default();
            cfg.hard_coded_contacts.push(crust0_info.clone());
            Crust::with_config(tx1, cfg)
        };
        crust1.start_listening();

        let crust1_port = unwrap!(crust1.our_localhost_connection_info())
            .peer_addr
            .port();
        let crust1_echoed_port = unwrap!(crust1.our_global_connection_info())
            .peer_addr
            .port();

        assert_eq!(crust1_port, crust1_echoed_port);
    }

    #[test]
    fn dropping_crust_handle_gracefully_shutsdown_event_loop() {
        let (tx, _rx) = mpsc::channel();
        let mut crust = Crust::new(tx);
        crust.start_listening();
    }

    #[test]
    fn multistreaming_and_no_head_of_queue_blocking_with_prior_connect() {
        // FIXME make these ports random and obtain them
        const CRUST0_PORT: u16 = 55113;
        const CRUST1_PORT: u16 = 56003;

        test_multistreaming(true, CRUST0_PORT, CRUST1_PORT);
    }

    #[test]
    fn multistreaming_and_no_head_of_queue_blocking_without_prior_connect() {
        // FIXME make these ports random and obtain them
        const CRUST0_PORT: u16 = 55213;
        const CRUST1_PORT: u16 = 56203;

        test_multistreaming(false, CRUST0_PORT, CRUST1_PORT);
    }

    fn test_multistreaming(should_connect: bool, crust0_port: u16, crust1_port: u16) {
        let (tx0, rx0) = mpsc::channel();
        let (mut crust0, crust0_cert_der) = {
            let our_complete_cert = SerialisableCertificate::default();
            let cert_der = our_complete_cert.cert_der.clone();

            let mut cfg: Config = Default::default();
            cfg.our_complete_cert = Some(our_complete_cert);
            cfg.port = Some(crust0_port);

            (Crust::with_config(tx0, cfg), cert_der)
        };

        let crust0_info = CrustInfo {
            peer_addr: unwrap!(format!("127.0.0.1:{}", crust0_port).parse()),
            peer_cert_der: crust0_cert_der,
        };
        let crust0_addr = crust0_info.peer_addr;

        let (tx1, rx1) = mpsc::channel();
        let mut crust1 = {
            let mut cfg: Config = Default::default();
            cfg.hard_coded_contacts.push(crust0_info.clone());
            cfg.port = Some(crust1_port);
            Crust::with_config(tx1, cfg)
        };

        let crust1_addr = unwrap!(SocketAddr::from_str(&format!("127.0.0.1:{}", crust1_port)));

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

        crust0.start_listening();
        crust1.start_listening();

        if should_connect {
            crust1.connect_to(crust0_info.clone());
        }

        let crust1_info = unwrap!(crust1.our_global_connection_info());
        assert_eq!(crust1_port, crust1_info.peer_addr.port());

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
}
