// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::config::OurType;
use crate::connection::{
    BootstrapGroupMaker, BootstrapGroupRef, Connection, FromPeer, QConn, ToPeer,
};
use crate::context::ctx_mut;
use crate::error::QuicP2pError;
use crate::event::Event;
use crate::peer_config;
use crate::utils::{self, Token};
use crate::wire_msg::{Handshake, WireMsg};
use crate::{communicate, NodeInfo, Peer, R};
use futures::{
    future::{FutureExt, TryFutureExt},
    select,
};
use log::{debug, info, trace};
use std::mem;
use std::net::SocketAddr;

/// Connect to the given peer
pub fn connect_to(
    peer_info: NodeInfo,
    send_after_connect: Option<(WireMsg, Token)>,
    bootstrap_group_maker: Option<&BootstrapGroupMaker>,
) -> R<()> {
    debug!("Connecting to {}", peer_info);

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

        let (terminator, mut rx) = utils::connect_terminator();

        let conn = c.connections.entry(peer_addr).or_insert_with(|| {
            Connection::new(
                peer_addr,
                event_tx.clone(),
                bootstrap_group_maker
                    .map(|m| m.add_member_and_get_group_ref(peer_addr, terminator.clone())),
            )
        });

        if conn.to_peer.is_no_connection() {
            // TODO see if this can be the default from-peer for OurType::Client
            if c.our_type == OurType::Client {
                if !conn.from_peer.is_no_connection() {
                    panic!("Logic Error - cannot expect Network to reverse connect to a client");
                }
                conn.from_peer = FromPeer::NotNeeded;
            }

            // If we already had an incoming from someone we are trying to bootstrap off
            if conn.bootstrap_group_ref.is_none() {
                conn.bootstrap_group_ref = bootstrap_group_maker
                    .map(|b| b.add_member_and_get_group_ref(peer_addr, terminator.clone()));
            }

            let mut pending_sends: Vec<_> = Default::default();
            if let Some(pending_send) = send_after_connect {
                pending_sends.push(pending_send);
            }
            conn.to_peer = ToPeer::Initiated {
                terminator: terminator.clone(),
                peer_addr,
                peer_cert_der: peer_info.peer_cert_der,
                pending_sends,
                event_tx,
            };

            let connecting = c
                .quic_ep()
                .connect_with(peer_cfg, &peer_addr, "MaidSAFE.net")?;

            let _ = tokio::spawn(async move {
                select! {
                    // Terminator leaf
                    _ = rx.recv().fuse() => {
                        handle_connect_err(peer_addr, &QuicP2pError::ConnectionCancelled);
                    },
                    // New connection
                    new_peer_conn_res = connecting.fuse() => {
                        handle_new_connection_res(peer_addr, new_peer_conn_res);
                    },
                }
            });

            Ok(())
        } else {
            Err(QuicP2pError::DuplicateConnectionToPeer { peer_addr })
        }
    });

    if let Err(e) = r.as_ref() {
        handle_connect_err(peer_addr, e);
    }

    r
}

fn handle_new_connection_res(
    peer_addr: SocketAddr,
    new_peer_conn_res: Result<quinn::NewConnection, quinn::ConnectionError>,
) {
    let (driver, q_conn, uni_streams, bi_streams) = match new_peer_conn_res {
        Ok(quinn::NewConnection {
            driver,
            connection,
            uni_streams,
            bi_streams,
            ..
        }) => (driver, QConn::from(connection), uni_streams, bi_streams),
        Err(e) => return handle_connect_err(peer_addr, &From::from(e)),
    };
    let _ = tokio::spawn(driver.map_err(move |e| handle_connect_err(peer_addr, &From::from(e))));

    trace!("Successfully connected to peer: {}", peer_addr);

    let mut should_accept_incoming = false;
    let mut terminate_bootstrap_group: Option<BootstrapGroupRef> = None;

    ctx_mut(|c| {
        let conn = match c.connections.get_mut(&peer_addr) {
            Some(conn) => conn,
            None => {
                trace!(
                    "Made a successful connection to a peer we are no longer interested in or \
                     the peer had its connection to us servered which made us forget this peer: {}",
                    peer_addr
                );
                return;
            }
        };

        let mut to_peer_prev = mem::take(&mut conn.to_peer);
        let (peer_cert_der, pending_sends) = match to_peer_prev {
            ToPeer::Initiated {
                ref mut peer_cert_der,
                ref mut pending_sends,
                ..
            } => (mem::take(peer_cert_der), mem::take(pending_sends)),
            // TODO analyse if this is actually reachable in some wierd case where things were in
            // the event loop and resolving now etc
            x => unreachable!(
                "TODO We can handle new connection only because it was previously \
                 initiated: {:?}",
                x
            ),
        };

        let node_info = NodeInfo {
            peer_addr,
            peer_cert_der: peer_cert_der.clone(),
        };
        if conn.we_contacted_peer {
            c.bootstrap_cache.add_peer(node_info.clone());
        }

        match conn.from_peer {
            FromPeer::NoConnection => {
                communicate::write_to_peer_connection(
                    Peer::Node {
                        node_info: node_info.clone(),
                    },
                    &q_conn,
                    WireMsg::Handshake(Handshake::Node {
                        cert_der: c.our_complete_cert.cert_der.clone(),
                    }),
                    0,
                );
            }
            FromPeer::NotNeeded => {
                communicate::write_to_peer_connection(
                    Peer::Node {
                        node_info: node_info.clone(),
                    },
                    &q_conn,
                    WireMsg::Handshake(Handshake::Client),
                    0,
                );

                let event = if let Some(bootstrap_group_ref) = conn.bootstrap_group_ref.take() {
                    terminate_bootstrap_group = Some(bootstrap_group_ref);
                    Event::BootstrappedTo {
                        node: node_info.clone(),
                    }
                } else {
                    Event::ConnectedTo {
                        peer: Peer::Node {
                            node_info: node_info.clone(),
                        },
                    }
                };

                if let Err(e) = c.event_tx.send(event) {
                    info!("Could not fire event: {:?}", e);
                }

                should_accept_incoming = true;
            }
            FromPeer::Established {
                ref mut pending_reads,
                ..
            } => {
                let event = if let Some(bootstrap_group_ref) = conn.bootstrap_group_ref.take() {
                    terminate_bootstrap_group = Some(bootstrap_group_ref);
                    Event::BootstrappedTo {
                        node: node_info.clone(),
                    }
                } else {
                    Event::ConnectedTo {
                        peer: node_info.clone().into(),
                    }
                };

                if let Err(e) = c.event_tx.send(event) {
                    info!("Could not fire event: {:?}", e);
                }

                for pending_read in pending_reads.drain(..) {
                    communicate::dispatch_wire_msg(
                        Peer::Node {
                            node_info: node_info.clone(),
                        },
                        &q_conn,
                        c.our_ext_addr_tx.take(),
                        &c.event_tx,
                        pending_read,
                        &mut c.bootstrap_cache,
                        conn.we_contacted_peer,
                    );
                }
            }
        }

        for (pending_send, token) in pending_sends {
            communicate::write_to_peer_connection(
                Peer::Node {
                    node_info: node_info.clone(),
                },
                &q_conn,
                pending_send,
                token,
            );
        }

        conn.to_peer = ToPeer::Established {
            peer_cert_der,
            q_conn,
        };
    });

    if let Some(bootstrap_group_ref) = terminate_bootstrap_group {
        bootstrap_group_ref.terminate_group(true);
    }

    if should_accept_incoming {
        communicate::read_from_peer(peer_addr, uni_streams, bi_streams);
    }
}

fn handle_connect_err(peer_addr: SocketAddr, e: &QuicP2pError) {
    debug!(
        "Error connecting to peer {}: {:?} - Details: {}",
        peer_addr, e, e
    );

    if let QuicP2pError::DuplicateConnectionToPeer { .. } = e {
        return;
    }

    ctx_mut(|c| {
        if let Some(conn) = c.connections.remove(&peer_addr) {
            if !conn.from_peer.is_no_connection() {
                info!(
                    "Peer {} has a connection to us but we couldn't connect to it. \
                     All connections to this peer will now be severed.",
                    peer_addr
                );
            }
        }
    })
}
