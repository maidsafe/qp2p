// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::config::OurType;
use crate::context::{ctx_mut, Connection, FromPeer, ToPeer};
use crate::error::Error;
use crate::event::Event;
use crate::peer_config;
use crate::wire_msg::{Handshake, WireMsg};
use crate::{communicate, NodeInfo, Peer, R};
use std::mem;
use std::net::SocketAddr;
use tokio::prelude::Future;
use tokio::runtime::current_thread;

/// Connect to the give peer
pub fn connect_to(peer_info: NodeInfo, send_after_connect: Option<WireMsg>) -> R<()> {
    let peer_addr = peer_info.peer_addr;

    let peer_cfg = match peer_config::new_client_cfg(&peer_info.peer_cert_der) {
        Ok(cfg) => cfg,
        Err(e) => {
            handle_connect_err(peer_addr, &e);
            return Err(e);
        }
    };

    let r = ctx_mut(|c| {
        let event_tx = c.event_tx.clone();
        let conn = c
            .connections
            .entry(peer_addr)
            .or_insert_with(|| Connection::new(peer_addr, event_tx));
        if conn.to_peer.is_no_connection() {
            // TODO see if this can be the default from-peer for OurType::Client
            if c.our_type == OurType::Client {
                if !conn.from_peer.is_no_connection() {
                    panic!("Logic Error - cannot expect Network to reverse connect to a client");
                }
                conn.from_peer = FromPeer::NotNeeded;
            }
            let mut pending_sends: Vec<_> = Default::default();
            if let Some(pending_send) = send_after_connect {
                pending_sends.push(pending_send);
            }
            conn.to_peer = ToPeer::Initiated {
                peer_cert_der: peer_info.peer_cert_der,
                pending_sends,
            };
            let peer_addr = peer_addr;
            c.quic_ep()
                .connect_with(peer_cfg, &peer_addr, "MaidSAFE.net")
                .map_err(Error::from)
                .map(move |new_client_conn_fut| {
                    let leaf = new_client_conn_fut.then(move |new_peer_conn_res| {
                        handle_new_connection_res(peer_addr, new_peer_conn_res);
                        Ok(())
                    });

                    current_thread::spawn(leaf);
                })
        } else {
            Err(Error::DuplicateConnectionToPeer(peer_addr))
        }
    });

    if let Err(e) = r.as_ref() {
        handle_connect_err(peer_addr, e);
    }

    r
}

fn handle_new_connection_res(
    peer_addr: SocketAddr,
    new_peer_conn_res: Result<
        (
            quinn::ConnectionDriver,
            quinn::Connection,
            quinn::IncomingStreams,
        ),
        quinn::ConnectionError,
    >,
) {
    let (conn_driver, q_conn, incoming_streams) = match new_peer_conn_res {
        Ok((conn_driver, q_conn, incoming_streams)) => (conn_driver, q_conn, incoming_streams),
        Err(e) => return handle_connect_err(peer_addr, &From::from(e)),
    };
    current_thread::spawn(
        conn_driver.map_err(move |e| handle_connect_err(peer_addr, &From::from(e))),
    );

    println!("Successfully connected to peer: {}", peer_addr);

    let mut should_accept_incoming = false;

    ctx_mut(|c| {
        let conn = match c.connections.get_mut(&peer_addr) {
            Some(conn) => conn,
            None => {
                println!(
                    "Made a successful connection to a peer we are no longer interested in or \
                     the peer had its connection to us servered which made us forget this peer: {}",
                    peer_addr
                );
                return;
            }
        };

        let mut to_peer_prev = mem::replace(&mut conn.to_peer, Default::default());
        let (peer_cert_der, pending_sends) = match to_peer_prev {
            ToPeer::Initiated {
                ref mut peer_cert_der,
                ref mut pending_sends,
            } => (
                mem::replace(peer_cert_der, Default::default()),
                mem::replace(pending_sends, Default::default()),
            ),
            // TODO analyse if this is actually reachable in some wierd case where things were in
            // the event loop and resolving now etc
            x => unreachable!(
                "TODO We can handle new connection only because it was previously \
                 initiated: {:?}",
                x
            ),
        };

        match conn.from_peer {
            FromPeer::NoConnection => {
                communicate::write_to_peer_connection(
                    peer_addr,
                    &q_conn,
                    WireMsg::Handshake(Handshake::Node {
                        cert_der: c.our_complete_cert.cert_der.clone(),
                    }),
                );
            }
            FromPeer::NotNeeded => {
                communicate::write_to_peer_connection(
                    peer_addr,
                    &q_conn,
                    WireMsg::Handshake(Handshake::Client),
                );

                let node_info = NodeInfo {
                    peer_addr,
                    peer_cert_der: peer_cert_der.clone(),
                };
                let peer = Peer::Node { node_info };

                if let Err(e) = c.event_tx.send(Event::ConnectedTo { peer }) {
                    println!("Could not fire event: {:?}", e);
                }

                should_accept_incoming = true;
            }
            FromPeer::Established {
                ref mut pending_reads,
                ..
            } => {
                let node_info = NodeInfo {
                    peer_addr,
                    peer_cert_der: peer_cert_der.clone(),
                };
                let peer = Peer::Node { node_info };

                if let Err(e) = c.event_tx.send(Event::ConnectedTo { peer }) {
                    println!("Could not fire event: {:?}", e);
                }

                for pending_read in pending_reads.drain(..) {
                    communicate::dispatch_wire_msg(
                        peer_addr,
                        &q_conn,
                        c.our_ext_addr_tx.take(),
                        &c.event_tx,
                        pending_read,
                    );
                }
            }
        }

        for pending_send in pending_sends {
            communicate::write_to_peer_connection(peer_addr, &q_conn, pending_send);
        }

        conn.to_peer = ToPeer::Established {
            peer_cert_der,
            q_conn,
        };
    });

    if should_accept_incoming {
        communicate::read_from_peer(peer_addr, incoming_streams);
    }
}

fn handle_connect_err(peer_addr: SocketAddr, e: &Error) {
    println!(
        "Error connecting to peer {}: {:?} - Details: {}",
        peer_addr, e, e
    );

    if let Error::DuplicateConnectionToPeer(_) = e {
        return;
    }

    ctx_mut(|c| {
        if let Some(conn) = c.connections.remove(&peer_addr) {
            if !conn.from_peer.is_no_connection() {
                println!(
                    "Peer {} has a connection to us but we couldn't connect to it. \
                     All connections to this peer will now be severed.",
                    peer_addr
                );
            }
        }
    })
}
