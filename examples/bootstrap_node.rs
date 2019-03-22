//! Bootstrap node which acts as a relay for other client nodes. It collects the info of multiple
//! client nodes and relays it to all remaining connected nodes, hence allows them all to connect
//! with each other.
//!
//! Usage:
//! ```
//! $ RUST_LOG=bootstrap_node=info cargo run --example bootstrap_node
//! ```

extern crate config_file_handler;
#[macro_use]
extern crate log;
#[macro_use]
extern crate unwrap;
#[macro_use]
extern crate serde_derive;

mod common;
use common::Rpc;

use using_quinn::{Config, Crust, CrustInfo, Event, SerialisableCertificate};

use bincode;
use config_file_handler::FileHandler;
use env_logger;
use serde_json;
use std::collections::HashMap;
use std::sync::mpsc::channel;
use std::{io, net::Ipv4Addr};

#[derive(Serialize, Deserialize)]
struct BootstrapNodeConfig {
    ip: Ipv4Addr,
    port: u16,
}

impl Default for BootstrapNodeConfig {
    fn default() -> Self {
        BootstrapNodeConfig {
            ip: unwrap!("127.0.0.1".parse()),
            port: 5000,
        }
    }
}

fn main() -> Result<(), io::Error> {
    env_logger::init();

    // Initialise configuration
    let cfg_file_handler = FileHandler::<BootstrapNodeConfig>::new("bootstrap.config", true)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    let bootstrap_node_config = cfg_file_handler
        .read_file()
        .unwrap_or_else(|_e| BootstrapNodeConfig::default());

    // Initialise Crust
    let (ev_tx, ev_rx) = channel();

    let (mut crust, our_cert_der) = {
        let our_complete_cert = SerialisableCertificate::default();
        let cert_der = our_complete_cert.cert_der.clone();
        (
            Crust::with_config(
                ev_tx,
                Config {
                    our_complete_cert: Some(our_complete_cert),
                    port: Some(bootstrap_node_config.port),
                    ..Default::default()
                },
            ),
            cert_der,
        )
    };
    crust.start_listening();

    info!("Crust started on port {}", bootstrap_node_config.port);

    // TODO(povilas): make our_connection_info use IGD crate when no stun servers configured
    let our_conn_info = CrustInfo {
        peer_addr: unwrap!(format!("127.0.0.1:{}", bootstrap_node_config.port).parse()),
        peer_cert_der: our_cert_der,
    };

    // let our_conn_info = unwrap!(crust.our_connection_info());
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

    Ok(())
}
