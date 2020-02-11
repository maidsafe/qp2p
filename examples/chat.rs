// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

//! Basic chat like example that demonstrates how to connect with peers and exchange data.

mod common;

use bytes::Bytes;
use common::{new_unbounded_channels, EventReceivers};
use quic_p2p::{Builder, Config, Event, Peer, QuicP2p};
use rand::{distributions::Standard, Rng};
use rustyline::config::Configurer;
use rustyline::error::ReadlineError;
use rustyline::Editor;
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
};
use structopt::StructOpt;
use unwrap::unwrap;

struct PeerList {
    peers: Vec<Peer>,
}

impl PeerList {
    fn new() -> Self {
        Self { peers: Vec::new() }
    }

    fn insert_from_str(&mut self, input: &str) -> Result<(), &str> {
        parse_peer(input).map(|v| self.insert(v))
    }

    fn insert(&mut self, peer: Peer) {
        if !self.peers.contains(&peer) {
            self.peers.push(peer)
        }
    }

    fn remove(&mut self, peer_idx: usize) -> Result<Peer, &str> {
        if peer_idx < self.peers.len() {
            Ok(self.peers.remove(peer_idx))
        } else {
            Err("Index out of bounds")
        }
    }

    fn get(&self, peer_idx: usize) -> Option<&Peer> {
        self.peers.get(peer_idx)
    }

    fn list(&self) {
        for (idx, peer) in self.peers.iter().enumerate() {
            match peer {
                Peer::Client(addr) => println!("{:3}: client:{}", idx, addr),
                Peer::Node(addr) => println!("{:3}: node:{}", idx, addr),
            }
        }
    }
}

/// This chat app connects two machines directly without intermediate servers and allows
/// to exchange messages securely. All the messages are end to end encrypted.
#[derive(Debug, StructOpt)]
struct CliArgs {
    #[structopt(flatten)]
    quic_p2p_opts: Config,
}

fn main() {
    let CliArgs { quic_p2p_opts } = CliArgs::from_args();
    let (ev_tx, ev_rx) = new_unbounded_channels();

    let mut qp2p = unwrap!(Builder::new(ev_tx).with_config(quic_p2p_opts).build());

    print_logo();
    println!("Type 'help' to get started.");

    let peerlist = Arc::new(Mutex::new(PeerList::new()));
    let rx_thread = handle_qp2p_events(ev_rx, peerlist.clone());

    let mut rl = Editor::<()>::new();
    rl.set_auto_add_history(true);
    'outer: loop {
        match rl.readline(">> ") {
            Ok(line) => {
                let mut args = line.trim().split_whitespace();
                let cmd = if let Some(cmd) = args.next() {
                    cmd
                } else {
                    continue 'outer;
                };
                let mut peerlist = peerlist.lock().unwrap();
                let result = match cmd {
                    "ourinfo" => {
                        print_ourinfo(&mut qp2p);
                        Ok(())
                    }
                    "addpeer" => peerlist.insert_from_str(&args.collect::<Vec<_>>().join(" ")),
                    "listpeers" => {
                        peerlist.list();
                        Ok(())
                    }
                    "delpeer" => args
                        .next()
                        .ok_or("Missing index argument")
                        .and_then(|idx| idx.parse().or(Err("Invalid index argument")))
                        .and_then(|idx| peerlist.remove(idx))
                        .and(Ok(())),
                    "send" => on_cmd_send(&mut args, &peerlist, &mut qp2p),
                    "sendrand" => on_cmd_send_rand(&mut args, &peerlist, &mut qp2p),
                    "quit" | "exit" => break 'outer,
                    "help" => {
                        println!(
                            "Commands: ourinfo, addpeer, listpeers, delpeer, send, quit, exit, help"
                        );
                        Ok(())
                    }
                    _ => Err("Unknown command"),
                };
                if let Err(msg) = result {
                    println!("Error: {}", msg);
                }
            }
            Err(ReadlineError::Eof) | Err(ReadlineError::Interrupted) => break 'outer,
            Err(e) => {
                println!("Error reading line: {}", e);
            }
        }
    }

    drop(qp2p);
    rx_thread.join().unwrap();
}

fn on_cmd_send<'a>(
    mut args: impl Iterator<Item = &'a str>,
    peer_list: &PeerList,
    qp2p: &mut QuicP2p,
) -> Result<(), &'static str> {
    args.next()
        .ok_or("Missing index argument")
        .and_then(|idx| idx.parse().or(Err("Invalid index argument")))
        .and_then(|idx| peer_list.get(idx).ok_or("Index out of bounds"))
        .map(|peer| {
            let msg = Bytes::from(args.collect::<Vec<_>>().join(" ").as_bytes());
            // TODO: handle tokens properly. Currently just hardcoding to 0 in example
            qp2p.send(peer.clone(), msg, 0);
        })
}

/// Sends random data of given size to given peer.
/// Usage: "sendrand <peer_index> <bytes>
fn on_cmd_send_rand<'a>(
    mut args: impl Iterator<Item = &'a str>,
    peer_list: &PeerList,
    qp2p: &mut QuicP2p,
) -> Result<(), &'static str> {
    args.next()
        .ok_or("Missing index argument")
        .and_then(|idx| idx.parse().or(Err("Invalid index argument")))
        .and_then(|idx| peer_list.get(idx).ok_or("Index out of bounds"))
        .and_then(|peer| {
            args.next()
                .ok_or("Missing bytes count")
                .and_then(|bytes| bytes.parse().or(Err("Invalid bytes count argument")))
                .map(|bytes_to_send| (peer, bytes_to_send))
        })
        .map(|(peer, bytes_to_send)| {
            let data = Bytes::from(random_vec(bytes_to_send));
            // TODO: handle tokens properly. Currently just hardcoding to 0 in example
            qp2p.send(peer.clone(), data, 0)
        })
}

fn handle_qp2p_events(event_rx: EventReceivers, peer_list: Arc<Mutex<PeerList>>) -> JoinHandle<()> {
    thread::spawn(move || {
        for event in event_rx.iter() {
            match event {
                Event::ConnectedTo { peer } => unwrap!(peer_list.lock()).insert(peer),
                Event::NewMessage { peer, msg } => {
                    if msg.len() > 512 {
                        println!("[{}] received bytes: {}", peer.peer_addr(), msg.len());
                    } else {
                        println!(
                            "[{}] {}",
                            peer.peer_addr(),
                            unwrap!(String::from_utf8(msg.to_vec()))
                        );
                    }
                }
                event => println!("Unexpected quic-p2p event: {:?}", event),
            }
        }
    })
}

fn parse_peer(input: &str) -> Result<Peer, &'static str> {
    if input.starts_with("client:") {
        return Ok(Peer::Client(parse_socket_addr(&input[7..])?));
    }

    if input.starts_with("node:") {
        return Ok(Peer::Node(parse_socket_addr(&input[5..])?));
    }

    Err("Invalid peer (valid examples: \"client:1.2.3.4:5678\", \"node:8.7.6.5:4321\", ...)")
}

fn parse_socket_addr(input: &str) -> Result<SocketAddr, &'static str> {
    input.parse().map_err(|_| "Invalid socket address")
}

fn print_ourinfo(qp2p: &mut QuicP2p) {
    let ourinfo = match qp2p.our_connection_info() {
        Ok(addr) => addr,
        Err(e) => {
            println!("Error getting ourinfo: {}", e);
            return;
        }
    };

    println!("Our info: {}", ourinfo);
}

fn print_logo() {
    println!(
        r#"

             _                  ____                _           _
  __ _ _   _(_) ___        _ __|___ \ _ __      ___| |__   __ _| |_
 / _` | | | | |/ __| ____ | '_ \ __) | '_ \    / __| '_ \ / _` | __|
| (_| | |_| | | (__ |____|| |_) / __/| |_) |  | (__| | | | (_| | |_
 \__, |\__,_|_|\___|      | .__/_____| .__/    \___|_| |_|\__,_|\__|
    |_|                   |_|        |_|

  "#
    );
}

fn random_vec(size: usize) -> Vec<u8> {
    rand::thread_rng()
        .sample_iter(&Standard)
        .take(size)
        .collect()
}
