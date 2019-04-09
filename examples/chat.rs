// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

//! Basic chat like example that demonstrates how to connect with peers and exchange data.

#[macro_use]
extern crate unwrap;

use std::net::{IpAddr};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::thread;

use bytes::Bytes;
use clap::{App, Arg};
use rustyline::config::Configurer;
use rustyline::error::ReadlineError;
use rustyline::Editor;
use serde_json;

use quic_p2p::{Builder, Config, Event, Peer, QuicP2p};

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

#[derive(Debug)]
struct CliArgs {
    port: Option<u16>,
    our_ip: Option<IpAddr>,
}

fn main() {
    let cli_args = parse_cli_args();
    let (ev_tx, ev_rx) = channel();

    let mut qp2p = unwrap!(Builder::new(ev_tx)
        .with_config(Config {
            port: cli_args.port,
            ip: cli_args.our_ip,
            ..Default::default()
        },)
        .build());

    print_logo();
    println!("Type 'help' to get started.");

    let peerlist = Arc::new(Mutex::new(PeerList::new()));
    let peerlist2 = peerlist.clone();

    let rx_thread = thread::spawn(move || {
        let peerlist = peerlist2;
        for event in ev_rx.iter() {
            match event {
                Event::ConnectedTo { peer } => peerlist.lock().unwrap().insert(peer),
                Event::NewMessage { peer_addr, msg } => {
                    println!(
                        "[{}] {}",
                        peer_addr,
                        unwrap!(String::from_utf8(msg.to_vec()))
                    );
                }
                event => println!("{:?}", event),
            }
        }
    });

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
                    "ourinfo" => Ok(print_ourinfo(&mut qp2p)),
                    "addpeer" => peerlist
                        .insert_from_json(&args.collect::<Vec<_>>().join(" "))
                        .and(Ok(())),
                    "listpeers" => Ok(peerlist.list()),
                    "delpeer" => args
                        .next()
                        .ok_or("Missing index argument")
                        .and_then(|idx| idx.parse().or(Err("Invalid index argument")))
                        .and_then(|idx| peerlist.remove(idx))
                        .and(Ok(())),
                    "send" => args
                        .next()
                        .ok_or("Missing index argument")
                        .and_then(|idx| idx.parse().or(Err("Invalid index argument")))
                        .and_then(|idx| peerlist.get(idx).ok_or("Index out of bounds"))
                        .map(|peer| {
                            // FIXME: I've unwrapped this due to API changes - pls handle
                            // appropriately
                            qp2p.send(
                                peer.clone(),
                                Bytes::from(
                                    args.collect::<Vec<_>>().join(" ").as_bytes().to_owned(),
                                ),
                            )
                        }),
                    "quit" | "exit" => break 'outer,
                    "help" => Ok(println!(
                        "Commands: ourinfo, addpeer, listpeers, delpeer, send, quit, exit, help"
                    )),
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

fn print_ourinfo(qp2p: &mut QuicP2p) {
    let ourinfo = match qp2p.our_connection_info() {
        Ok(ourinfo) => ourinfo,
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

fn parse_cli_args() -> CliArgs {
    let matches = App::new("Simple chat app built on Crust")
        .about(
            "This chat app connects two machines directly without intermediate servers and allows \
             to exchange messages securely. All the messages are end to end encrypted.",
        )
        .arg(
            Arg::with_name("listening_port")
                .help(
                    "Optional server Crust will be listening for incoming connections ON. If \
                     unspecified, random port will be used.",
                )
                .short("p")
                .value_name("PORT")
                .takes_value(true)
                .validator(|v| v.parse::<u16>().and(Ok(())).or(Err("Invalid port".into()))),
        )
        .arg(
            Arg::with_name("our_ip")
                .help(
                    "Our IP address to use when constructing our connection info. If unspecified \
                     , Crust will try to use bootstrap node to determine our IP.",
                )
                .short("a")
                .long("our-ip")
                .value_name("OUR_IP")
                .takes_value(true),
        )
        .get_matches();

    let port = matches
        .value_of("listening_port")
        .and_then(|v| v.parse().ok());
    let our_ip = matches.value_of("our_ip").map(|addr| unwrap!(addr.parse()));

    CliArgs { port, our_ip }
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
