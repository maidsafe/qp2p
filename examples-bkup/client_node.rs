//! This example connects to bootstrap node, waits for other client node contacts and starts
//! communicating with them.
//!
//! Usage:
//! ```
//! $ RUST_LOG=client_node=info cargo run --example client_node -- -b '{"peer_addr":
//! "127.0.0.1:5000","peer_cert_der":[48,130,..]}'
//! ```

#[macro_use]
extern crate log;
#[macro_use]
extern crate unwrap;

mod common;
use common::Rpc;

use using_quinn::{Config, Crust, CrustInfo, Event};

use clap::{self, App, Arg};
use crc::crc32;
use env_logger;
use rand::{self, RngCore};
use serde_json;
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::mpsc::{channel, Receiver};

#[derive(Debug)]
struct CliArgs {
    bootstrap_node_info: CrustInfo,
}

struct ClientNode {
    crust: Crust,
    bootstrap_node_info: CrustInfo,
    /// It's optional just to fight the borrow checker.
    event_rx: Option<Receiver<Event>>,
    /// Other nodes we will be communicating with.
    client_nodes: HashSet<CrustInfo>,
    our_cert: Vec<u8>,
    sent_messages: usize,
    received_messages: usize,
    /// Message to send
    msg: Vec<u8>,
}

fn main() {
    env_logger::init();
    let args = match parse_cli_args() {
        Ok(args) => args,
        Err(e) => e.exit(),
    };
    ClientNode::new(args.bootstrap_node_info).run();
}

impl ClientNode {
    fn new(bootstrap_node_info: CrustInfo) -> Self {
        let (event_tx, event_rx) = channel();
        let crust = Crust::with_config(
            event_tx,
            Config {
                port: Some(0),
                hard_coded_contacts: vec![bootstrap_node_info.clone()],
                idle_timeout: Some(120),
                ..Default::default()
            },
        );
        let msg = random_data_with_hash(4 * 1024);
        assert!(hash_correct(&msg));
        Self {
            crust,
            bootstrap_node_info,
            msg,
            event_rx: Some(event_rx),
            client_nodes: Default::default(),
            our_cert: Default::default(),
            sent_messages: 0,
            received_messages: 0,
        }
    }

    /// Blocks and reacts to crust events.
    fn run(&mut self) {
        self.crust.start_listening();
        info!("Crust started");

        self.our_cert = self.crust.our_certificate_der();

        // this dummy send will trigger connection
        self.crust
            .send(self.bootstrap_node_info.clone(), vec![1, 2, 3]);

        self.poll_crust_events();
    }

    fn poll_crust_events(&mut self) {
        let event_rx = unwrap!(self.event_rx.take());
        for event in event_rx.iter() {
            match event {
                Event::ConnectedTo { crust_info } => self.on_connect(crust_info),
                Event::NewMessage { peer_addr, msg } => self.on_msg_receive(peer_addr, msg),
                event => warn!("Unexpected event: {:?}", event),
            }
        }
    }

    fn on_connect(&mut self, peer: CrustInfo) {
        info!("Connected with: {}", peer.peer_addr);

        if peer == self.bootstrap_node_info {
            info!("Connected to bootstrap node. Waiting for other node contacts...");
        } else if self.client_nodes.contains(&peer) {
            self.crust.send(peer, self.msg.clone());
            self.sent_messages += 1;
        }
    }

    fn on_msg_receive(&mut self, peer_addr: SocketAddr, msg: Vec<u8>) {
        if self.response_from_bootstrap_node(&peer_addr) {
            let msg: Rpc = unwrap!(bincode::deserialize(&msg));
            match msg {
                Rpc::StartTest(peers) => self.connect_to_peers(peers),
            }
        } else {
            debug!("Message received: {}", msg.len());
            assert!(hash_correct(&msg));
            self.received_messages += 1;

            if self.received_messages == self.client_nodes.len()
                && self.sent_messages == self.client_nodes.len()
            {
                println!("Done. All checks passed");
            }
        }
    }

    fn connect_to_peers(&mut self, peers: Vec<CrustInfo>) {
        for conn_info in peers {
            if conn_info.peer_cert_der != self.our_cert {
                self.crust.connect_to(conn_info.clone());
                self.client_nodes.insert(conn_info);
            }
        }
    }

    fn response_from_bootstrap_node(&self, peer_addr: &SocketAddr) -> bool {
        peer_addr == &self.bootstrap_node_info.peer_addr
    }
}

fn parse_cli_args() -> Result<CliArgs, clap::Error> {
    let matches = App::new("Crust client node example")
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
