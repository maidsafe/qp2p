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

use std::sync::mpsc::channel;
use std::thread;

use bytes::Bytes;
use clap::{App, Arg};
use rustyline::config::Configurer;
use rustyline::error::ReadlineError;
use rustyline::Editor;
use serde_json;

use std::sync::{Arc, Mutex};
use using_quinn::{Config, Crust, Event, Peer};

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

fn main() {
    let matches = App::new("chat")
        .arg(
            Arg::with_name("listening_port")
                .short("p")
                .value_name("PORT")
                .takes_value(true)
                .validator(|v| v.parse::<u16>().and(Ok(())).or(Err("Invalid port".into()))),
        )
        .get_matches();

    let port = matches
        .value_of("listening_port")
        .and_then(|v| v.parse().ok());

    let (ev_tx, ev_rx) = channel();

    let mut crust = Crust::with_config(
        ev_tx,
        Config {
            port,
            ..Default::default()
        },
    );

    crust.start_listening();

    println!("Crust started");

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
                    "ourinfo" => Ok(print_ourinfo(&mut crust)),
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
                            crust.send(
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

    drop(crust);
    rx_thread.join().unwrap();
}

fn print_ourinfo(crust: &mut Crust) {
    let ourinfo = match crust.our_connection_info() {
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
