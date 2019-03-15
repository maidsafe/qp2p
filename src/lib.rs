#[macro_use]
extern crate quick_error;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate unwrap;

pub use event::Event;

use crate::wire_msg::WireMsg;
use config::Config;
use context::{ctx_mut, initialise_ctx, Context};
use error::Error;
use event_loop::EventLoop;
use futures::Future;
use std::net::SocketAddr;
use std::sync::mpsc::{self, Sender};
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
    our_ext_addr: Option<SocketAddr>,
    el: EventLoop,
}

impl Crust {
    /// Create a new Crust instance
    pub fn new(event_tx: Sender<Event>) -> Self {
        let el = EventLoop::spawn();
        let cfg = Default::default();

        Self {
            event_tx,
            cfg,
            our_ext_addr: None,
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
        self.el.post(move || {
            let (ep, dr, incoming_connections) =
                unwrap!(quinn::Endpoint::new().bind(&format!("0.0.0.0:{}", port)));

            let ctx = Context::new(tx, ep);
            initialise_ctx(ctx);

            current_thread::spawn(dr.map_err(|e| println!("Error in quinn Driver: {:?}", e)));
            listener::listen(incoming_connections);
        });
    }

    /// Connect to the given peer. This will error out if the peer is already in the process of
    /// being connected to OR for any other connection failure reasons.
    pub fn connect_to(&mut self, peer_addr: SocketAddr) {
        self.el.post(move || {
            let _r = connect::connect_to(peer_addr);
        });
    }

    /// Send message to peer. If the peer is not connected, it will attempt to connect to it first
    /// and then send the message
    pub fn send(&mut self, peer_addr: SocketAddr, msg: Vec<u8>) {
        self.el
            .post(move || communicate::try_write_to_peer(peer_addr, WireMsg::UserMsg(msg)));
    }

    /// Get our connection info to give to others for them to connect to us
    pub fn our_connection_info(&mut self) -> SocketAddr {
        self.query_ip_echo_service()
    }

    fn query_ip_echo_service(&mut self) -> SocketAddr {
        if let Some(addr) = self.our_ext_addr {
            return addr;
        }

        let (tx, rx) = mpsc::channel();
        let ip_echo_server = self.cfg.hard_coded_contacts[0];

        self.el.post(move || {
            ctx_mut(|c| c.our_ext_addr_tx = Some(tx));
            communicate::try_write_to_peer(ip_echo_server, WireMsg::EndpointEchoReq)
        });

        unwrap!(rx.recv())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn dropping_crust_handle_gracefully_shutsdown_event_loop() {
        let (tx, _rx) = mpsc::channel();
        let mut crust = Crust::new(tx.clone());
        crust.start_listening();
    }
}
