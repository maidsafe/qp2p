// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

//! Bootstrap node which acts as a relay for other client nodes. It collects the info of multiple
//! client nodes and relays it to all remaining connected nodes, hence allows them all to connect
//! with each other.
//!
//! Usage:
//! ```
//! $ RUST_LOG=bootstrap_node=info cargo run --example bootstrap_node -- --expected_conns 1 --ip
//! "127.0.0.1"
//! ```

mod common;
use bincode;
use bytes::Bytes;
use common::{new_unbounded_channels, Rpc};
use env_logger;
use log::{error, info, warn};
use quic_p2p::{Builder, Config, Event, Peer};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io;
use structopt::StructOpt;
use unwrap::unwrap;

/// Configuration for the bootstrap node
#[derive(Serialize, Deserialize, StructOpt)]
pub struct BootstrapNodeConfig {
    /// A number of expected connections.
    /// Once this number is reached, we'll send a list of all connections to every connected peer.
    #[structopt(short, long)]
    expected_conns: usize,
    #[structopt(flatten)]
    quic_p2p_opts: Config,
}

fn main() -> Result<(), io::Error> {
    env_logger::init();

    // Initialise configuration
    let bootstrap_node_config = BootstrapNodeConfig::from_args();

    // Initialise QuicP2p
    let (ev_tx, ev_rx) = new_unbounded_channels();

    let mut qp2p = unwrap!(Builder::new(ev_tx)
        .with_config(bootstrap_node_config.quic_p2p_opts)
        .build());

    let our_addr = unwrap!(qp2p.our_connection_info());
    info!("QuicP2p started on {}", our_addr);

    println!("Our connection info: {}", our_addr);

    let expected_connections = bootstrap_node_config.expected_conns;
    let mut connected_peers = HashMap::new();
    let mut test_triggered = false;

    for event in ev_rx.iter() {
        match event {
            Event::ConnectedTo { peer } => {
                let peer_addr = match peer {
                    Peer::Node(node_addr) => node_addr,
                    Peer::Client(_) => panic!("In this example only Node peers are expected"),
                };
                let _ = connected_peers.insert(peer_addr, peer);
                if connected_peers.len() == expected_connections && !test_triggered {
                    info!(
                        "{} connections collected, triggering the test",
                        expected_connections
                    );
                    let contacts: Vec<_> = connected_peers.values().cloned().collect();
                    let msg = Bytes::from(unwrap!(bincode::serialize(&Rpc::StartTest(contacts))));
                    for peer in connected_peers.values() {
                        // qp2p.send(Peer::Node { node_info: peer.clone() }, msg.clone());
                        // TODO: Demo tokens properly. Currently just putting 0 here.
                        // TODO: Also handle receiving `SentUserMessage` event
                        qp2p.send(peer.clone(), msg.clone(), 0);
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
