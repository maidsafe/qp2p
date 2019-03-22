//! Bootstrap node which acts as a relay for other client nodes. It collects the info of multiple
//! client nodes and relays it to all remaining connected nodes, hence allows them all to connect
//! with each other.
//!
//! Usage:
//! ```
//! $ RUST_LOG=bootstrap_node=info cargo run --example bootstrap_node
//! ```

#[macro_use]
extern crate log;
#[macro_use]
extern crate unwrap;
#[macro_use]
extern crate serde_derive;

mod common;
use common::Rpc;

use using_quinn::{Config, Crust, Event};

use bincode;
use env_logger;
use serde_json;
use std::collections::HashMap;
use std::sync::mpsc::channel;

fn main() {
    env_logger::init();

    let (ev_tx, ev_rx) = channel();

    let mut crust = Crust::with_config(
        ev_tx,
        Config {
            port: Some(5000), // TODO(povilas): make port configurable
            ..Default::default()
        },
    );
    crust.start_listening();
    info!("Crust started");

    // TODO(povilas): make our_connection_info use IGD crate when no stun servers configured
    let our_conn_info = unwrap!(crust.our_connection_info());
    println!(
        "Our connection info:\n{}\n",
        unwrap!(serde_json::to_string(&our_conn_info)),
    );

    // TODO(povilas): have an argument for this
    let expected_connections = 3;
    let mut connected_peers = HashMap::new();
    let mut test_triggered = false;

    for event in ev_rx.iter() {
        match event {
            Event::ConnectedTo { crust_info } => {
                let _ = connected_peers.insert(crust_info.peer_addr, crust_info);
                if connected_peers.len() == expected_connections && !test_triggered {
                    info!(
                        "{} connections collected, triggering the test",
                        expected_connections
                    );
                    let contacts: Vec<_> = connected_peers.values().cloned().collect();
                    let msg = unwrap!(bincode::serialize(&Rpc::StartTest(contacts)));
                    for peer in connected_peers.values() {
                        crust.send(peer.clone(), msg.clone());
                    }
                    test_triggered = true;
                } else if connected_peers.len() >= expected_connections {
                    error!("More than expected connections received");
                }
            }
            event => warn!("Unexpected event: {:?}", event),
        }
    }
}
