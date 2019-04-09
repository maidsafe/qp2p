//! This example connects to bootstrap node, waits for other client node contacts and starts
//! communicating with them.
//!
//! Usage:
//! ```
//! $ RUST_LOG=client_node=info cargo run --example client_node -- -b '{"peer_addr":
//! "127.0.0.1:5000","peer_cert_der":[48,130,..]}'
//! ```

extern crate bytes;
#[macro_use]
extern crate log;
#[macro_use]
extern crate unwrap;

mod common;

use bytes::Bytes;
use clap::{self, App, Arg};
use common::Rpc;
use crc::crc32;
use env_logger;
use rand::{self, RngCore};
use serde_json;
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::mpsc::{channel, Receiver};
use using_quinn::{Config, Event, Peer, PeerInfo};

#[derive(Debug)]
struct CliArgs {
    bootstrap_node_info: PeerInfo,
}

struct ClientNode {
    peer: Peer,
    bootstrap_node_info: PeerInfo,
    /// It's optional just to fight the borrow checker.
    event_rx: Option<Receiver<Event>>,
    /// Other nodes we will be communicating with.
    client_nodes: HashSet<PeerInfo>,
    our_cert: Vec<u8>,
    sent_messages: usize,
    received_messages: usize,
    /// Large message to send
    large_msg: Bytes,
    /// Smaller message to send
    small_msg: Bytes,
    peer_states: HashMap<SocketAddr, bool>,
}

const LARGE_MSG_SIZE: usize = 20 * 1024 * 1024; // 20 MB
const SMALL_MSG_SIZE: usize = 16 * 1024; // 16 KB

fn main() {
    env_logger::init();
    let args = match parse_cli_args() {
        Ok(args) => args,
        Err(e) => e.exit(),
    };
    ClientNode::new(args.bootstrap_node_info).run();
}

impl ClientNode {
    fn new(bootstrap_node_info: PeerInfo) -> Self {
        let (event_tx, event_rx) = channel();
        let peer = Peer::with_config(
            event_tx,
            Config {
                port: Some(0),
                hard_coded_contacts: vec![bootstrap_node_info.clone()],
                idle_timeout_msec: Some(0),
                ..Default::default()
            },
        );

        let large_msg = Bytes::from(random_data_with_hash(LARGE_MSG_SIZE));
        assert!(hash_correct(&large_msg));

        let small_msg = Bytes::from(random_data_with_hash(SMALL_MSG_SIZE));
        assert!(hash_correct(&small_msg));

        Self {
            peer,
            bootstrap_node_info,
            large_msg,
            small_msg,
            event_rx: Some(event_rx),
            client_nodes: Default::default(),
            our_cert: Default::default(),
            peer_states: Default::default(),
            sent_messages: 0,
            received_messages: 0,
        }
    }

    /// Blocks and reacts to peer events.
    fn run(&mut self) {
        self.peer.start_listening();
        info!("Peer started");

        self.our_cert = self.peer.our_certificate_der();

        // this dummy send will trigger connection
        self.peer
            .send(self.bootstrap_node_info.clone(), Bytes::from(vec![1, 2, 3]));

        self.poll_peer_events();
    }

    fn poll_peer_events(&mut self) {
        let event_rx = unwrap!(self.event_rx.take());
        for event in event_rx.iter() {
            match event {
                Event::ConnectedTo { peer_info } => self.on_connect(peer_info),
                Event::NewMessage { peer_addr, msg } => self.on_msg_receive(peer_addr, msg),
                event => warn!("Unexpected event: {:?}", event),
            }
        }
    }

    fn on_connect(&mut self, peer: PeerInfo) {
        info!("Connected with: {}", peer.peer_addr);

        if peer == self.bootstrap_node_info {
            info!("Connected to bootstrap node. Waiting for other node contacts...");
        } else if self.client_nodes.contains(&peer) {
            self.peer.send(peer.clone(), self.large_msg.clone());
            self.peer.send(peer, self.small_msg.clone());
            self.sent_messages += 1;
        }
    }

    fn on_msg_receive(&mut self, peer_addr: SocketAddr, msg: Bytes) {
        if self.response_from_bootstrap_node(&peer_addr) {
            let msg: Rpc = unwrap!(bincode::deserialize(&msg));
            match msg {
                Rpc::StartTest(peers) => self.connect_to_peers(peers),
            }
        } else {
            let small_msg_rcvd = self.peer_states.entry(peer_addr).or_insert(false);

            debug!("[{}] Message received: {}", peer_addr, msg.len());
            assert!(hash_correct(&msg));

            let payload_size = msg.len() - 4; // without the hash

            if payload_size == LARGE_MSG_SIZE {
                if !*small_msg_rcvd {
                    error!("[{}] Large message received before small", peer_addr);
                } else {
                    self.received_messages += 1;
                    debug!(
                        "Recv: {}/{}, Sent: {}/{}",
                        self.received_messages,
                        self.client_nodes.len(),
                        self.sent_messages,
                        self.client_nodes.len()
                    );

                    if self.received_messages == self.client_nodes.len()
                        && self.sent_messages == self.client_nodes.len()
                    {
                        info!("Done. All checks passed");
                    }
                }
            } else if payload_size == SMALL_MSG_SIZE {
                if *small_msg_rcvd {
                    error!("[{}] Small message received twice", peer_addr);
                }
                *small_msg_rcvd = true;
            }
        }
    }

    fn connect_to_peers(&mut self, peers: Vec<PeerInfo>) {
        for conn_info in peers {
            if conn_info.peer_cert_der != self.our_cert {
                self.peer.connect_to(conn_info.clone());
                self.client_nodes.insert(conn_info);
            }
        }
    }

    fn response_from_bootstrap_node(&self, peer_addr: &SocketAddr) -> bool {
        peer_addr == &self.bootstrap_node_info.peer_addr
    }
}

fn parse_cli_args() -> Result<CliArgs, clap::Error> {
    let matches = App::new("Peer client node example")
        .about(
            "Client node will be connecting to bootstrap node from which it will receive contacts
            of other client nodes. Then this node will connect with all other client nodes and
            try to exchange some data with them.",
        )
        .arg(
            Arg::with_name("bootstrap-node-info")
                .long("bootstrap-node-info")
                .short("b")
                .value_name("CONN_INFO")
                .help("Bootstrap node connection info.")
                .takes_value(true),
        )
        .get_matches();
    let bootstrap_node_info = matches
        .value_of("bootstrap-node-info")
        .map(|str_info| unwrap!(serde_json::from_str(str_info)))
        .ok_or_else(|| {
            clap::Error::with_description(
                "Bootstrap node connection info must be given",
                clap::ErrorKind::MissingRequiredArgument,
            )
        })?;
    Ok(CliArgs {
        bootstrap_node_info,
    })
}

fn random_data_with_hash(size: usize) -> Vec<u8> {
    let mut data = random_vec(size + 4);
    let hash = crc32::checksum_ieee(&data[4..]);
    // write hash in big endian
    data[0] = (hash >> 24) as u8;
    data[1] = ((hash >> 16) & 0xff) as u8;
    data[2] = ((hash >> 8) & 0xff) as u8;
    data[3] = (hash & 0xff) as u8;
    data
}

fn hash_correct(data: &[u8]) -> bool {
    let encoded_hash = ((data[0] as u32) << 24)
        | ((data[1] as u32) << 16)
        | ((data[2] as u32) << 8)
        | data[3] as u32;
    let actual_hash = crc32::checksum_ieee(&data[4..]);
    encoded_hash == actual_hash
}

#[allow(unsafe_code)]
fn random_vec(size: usize) -> Vec<u8> {
    let mut ret = Vec::with_capacity(size);
    unsafe { ret.set_len(size) };
    rand::thread_rng().fill_bytes(&mut ret[..]);
    ret
}
