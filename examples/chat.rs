// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

//! Basic chat like example that demonstrates how to connect with peers and exchange data.

use bytes::Bytes;
use crossbeam_channel as mpmc;
use quic_p2p::{Builder, Config, Event, Peer, QuicP2p};
use rand::{self, RngCore};
use rustyline::config::Configurer;
use rustyline::error::ReadlineError;
use rustyline::Editor;
use serde_json;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use structopt::StructOpt;
use unwrap::unwrap;

struct PeerList {
    peers: Vec<Peer>,
}

impl PeerList {
    fn new() -> Self {
        Self { peers: Vec::new() }
    }

    fn insert_from_json(&mut self, peer_json: &str) -> Result<(), &str> {
        serde_json::from_str(peer_json)
            .map(|v| self.insert(v))
            .map_err(|_| "Error parsing JSON")
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
            println!("{:3}: {}", idx, peer.peer_addr());
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
    let (ev_tx, ev_rx) = mpmc::unbounded();

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
                    "addpeer" => peerlist
                        .insert_from_json(&args.collect::<Vec<_>>().join(" "))
                        .and(Ok(())),
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
            qp2p.send(peer.clone(), msg);
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
            qp2p.send(peer.clone(), data)
        })
}

fn handle_qp2p_events(
    event_rx: mpmc::Receiver<Event>,
    peer_list: Arc<Mutex<PeerList>>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        for event in event_rx.iter() {
            match event {
                Event::ConnectedTo { peer } => unwrap!(peer_list.lock()).insert(peer),
                Event::NewMessage { peer_addr, msg } => {
                    if msg.len() > 512 {
                        println!("[{}] received bytes: {}", peer_addr, msg.len());
                    } else {
                        println!(
                            "[{}] {}",
                            peer_addr,
                            unwrap!(String::from_utf8(msg.to_vec()))
                        );
                    }
                }
                event => println!("Unexpected Crust event: {:?}", event),
            }
        }
    })
}

fn print_ourinfo(qp2p: &mut QuicP2p) {
    let ourinfo: Peer = match qp2p.our_connection_info() {
        Ok(ourinfo) => ourinfo.into(),
        Err(e) => {
            println!("Error getting ourinfo: {}", e);
            return;
        }
    };

    println!(
        "Our info:\n\n{}\n",
        serde_json::to_string(&ourinfo).unwrap()
    );
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

#[allow(unsafe_code)]
fn random_vec(size: usize) -> Vec<u8> {
    let mut ret = Vec::with_capacity(size);
    unsafe { ret.set_len(size) };
    rand::thread_rng().fill_bytes(&mut ret[..]);
    ret
}
