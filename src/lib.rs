#![allow(unused, dead_code)]

#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate unwrap;

pub use event::Event;

use config::Config;
use event_loop::{EventLoop, EventLoopMsg, EventLoopState};
use futures::Future;
use std::net::SocketAddr;
use std::sync::mpsc::Sender;
use tokio::runtime::current_thread;

mod config;
mod event;
mod event_loop;
mod listener;
mod wire_msg;

/// Main Crust instance to communicate with Crust
pub struct Crust {
    event_tx: Sender<Event>,
    cfg: Config,
    el: EventLoop,
}

impl Crust {
    /// Create a new Crust instance
    pub fn new(event_tx: Sender<Event>) -> Self {
        let el = EventLoop::spawn(event_tx.clone());
        let cfg = Default::default();

        Self { event_tx, cfg, el }
    }

    /// Start listener
    pub fn start_listening(&mut self) {
        let port = self.cfg.port.unwrap_or(0);
        self.post(move |el_state| {
            let (ep, dr, incoming_connections) =
                unwrap!(quinn::Endpoint::new().bind(&format!("0.0.0.0:{}", port)));
            current_thread::spawn(dr.map_err(|e| println!("Error in quinn Driver: {:?}", e)));
            assert!(el_state.insert_quic_endpoint(ep), "QUIC EP already created");
            listener::listen(el_state.clone(), incoming_connections);
        });
    }

    fn query_ip_echo_service(&mut self) -> SocketAddr {
        let ip_echo_server = self.cfg.hard_coded_contacts[0];
        unimplemented!()
    }

    fn post<F>(&mut self, f: F)
    where
        F: FnOnce(&EventLoopState) + Send + 'static,
    {
        EventLoop::post(self.el.tx(), f);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn dropping_crust_handle_gracefully_shutsdown_event_loop() {
        let (tx, _rx) = mpsc::channel();
        let _crust = Crust::new(tx.clone());
    }
}
