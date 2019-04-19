// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::bootstrap_cache::BootstrapCache;
use crate::connection::{Connection, FromPeer, ToPeer};
use crate::context::{ctx, ctx_mut};
use crate::error::Error;
use crate::event::Event;
use crate::utils::{self, QConn};
use crate::wire_msg::{Handshake, WireMsg};
use crate::{connect, NodeInfo};
use crate::{Peer, R};
use std::net::SocketAddr;
use std::sync::mpsc::Sender;
use tokio::prelude::{Future, Stream};
use tokio::runtime::current_thread;

/// Send message to peer. If the peer is not connected, it will attempt to connect to it first
/// and then send the message
pub fn try_write_to_peer(peer: Peer, msg: WireMsg) {
    let node_info = match peer {
        Peer::Client { peer_addr } => return write_to_peer(peer_addr, msg),
        Peer::Node { node_info } => node_info,
    };

    let connect_and_send = ctx_mut(|c| {
        let peer_addr = node_info.peer_addr;
        let event_tx = c.event_tx.clone();
        let conn = c
            .connections
            .entry(peer_addr)
            .or_insert_with(|| Connection::new(peer_addr, event_tx, None));

        match conn.to_peer {
            ToPeer::NoConnection => Some(msg),
            ToPeer::NotNeeded => {
                warn!("TODO We normally can't get here - ignoring");
                None
            }
            ToPeer::Initiated {
                ref peer_cert_der,
                ref mut pending_sends,
                ..
            } => {
                if *peer_cert_der != node_info.peer_cert_der {
                    info!("TODO Certificate we have for the peer already doesn't match with the \
                    one given - we should disconnect to such peers - something fishy going on.");
                }
                pending_sends.push(msg);
                None
            }
            ToPeer::Established { ref q_conn, .. } => {
                write_to_peer_connection(node_info.peer_addr, q_conn, msg);
                None
            }
        }
    });

    if connect_and_send.is_some() {
        let peer_addr = node_info.peer_addr;
        if let Err(e) = connect::connect_to(node_info, connect_and_send, None) {
            debug!(
                "Unable to connect to peer {} to be able to send message: {:?}",
                peer_addr, e
            );
        }
    }
}

/// This will fail if we don't have a connection to the peer or if the peer is in an invalid state
/// to be sent a message to.
pub fn write_to_peer(peer_addr: SocketAddr, msg: WireMsg) {
    ctx(|c| {
        let conn = match c.connections.get(&peer_addr) {
            Some(conn) => conn,
            None => return trace!("Asked to communicate with an unknown peer: {}", peer_addr),
        };

        match &conn.to_peer {
            ToPeer::NotNeeded => {
                if let FromPeer::Established { ref q_conn, .. } = conn.from_peer {
                    write_to_peer_connection(peer_addr, q_conn, msg);
                } else {
                    debug!(
                        "TODO We cannot communicate with someone we are not needing to connect to \
                         and they are not connected to us just now. Peer: {}",
                        peer_addr
                    );
                }
            }
            ToPeer::Established { ref q_conn, .. } => {
                write_to_peer_connection(peer_addr, q_conn, msg)
            }
            ToPeer::NoConnection | ToPeer::Initiated { .. } => {
                return debug!(
                    "Peer {} is in invalid state {:?} to be communicated to",
                    peer_addr, conn.to_peer
                );
            }
        }
    })
}

/// Write to the peer, given the QUIC connection to it
pub fn write_to_peer_connection(peer_addr: SocketAddr, conn: &QConn, wire_msg: WireMsg) {
    let leaf = conn
        .open_uni()
        .map_err(move |e| {
            utils::handle_communication_err(peer_addr, &From::from(e), "Open-Unidirectional")
        })
        .and_then(move |o_stream| {
            tokio::io::write_all(o_stream, wire_msg.into()).map_err(move |e| {
                utils::handle_communication_err(peer_addr, &From::from(e), "Write-All")
            })
        })
        .and_then(move |(o_stream, _): (_, bytes::Bytes)| {
            tokio::io::shutdown(o_stream).map_err(move |e| {
                utils::handle_communication_err(peer_addr, &From::from(e), "Shutdown-after-write")
            })
        })
        .map(|_| ());

    current_thread::spawn(leaf);
}

/// Listen for incoming streams containing peer messages and read them when available
pub fn read_from_peer(peer_addr: SocketAddr, incoming_streams: quinn::IncomingStreams) {
    let leaf = incoming_streams
        .map_err(move |e| {
            utils::handle_communication_err(peer_addr, &From::from(e), "Incoming streams failed");
        })
        .for_each(move |quic_stream| {
            read_peer_stream(peer_addr, quic_stream).map_err(|e| {
                debug!(
                    "Error in Incoming-streams while reading from peer {}: {:?} - {}.",
                    peer_addr, e, e
                )
            })
        });

    current_thread::spawn(leaf);
}

fn read_peer_stream(peer_addr: SocketAddr, quic_stream: quinn::NewStream) -> R<()> {
    let i_stream = match quic_stream {
        quinn::NewStream::Bi(_, _) => {
            let e = Error::BiDirectionalStreamAttempted(peer_addr);
            utils::handle_communication_err(peer_addr, &e, "Receiving Stream");
            return Err(e);
        }
        quinn::NewStream::Uni(uni) => uni,
    };

    let leaf = i_stream
        .read_to_end(ctx(|c| c.max_msg_size_allowed))
        .map_err(move |e| utils::handle_communication_err(peer_addr, &From::from(e), "Read-To-End"))
        .and_then(move |(_i_stream, raw)| {
            WireMsg::from_raw(raw.into())
                .map_err(|e| utils::handle_communication_err(peer_addr, &e, "Raw to WireMsg"))
                .map(|wire_msg| handle_wire_msg(peer_addr, wire_msg))
        });

    current_thread::spawn(leaf);

    Ok(())
}

/// Handle wire messages from peer
pub fn handle_wire_msg(peer_addr: SocketAddr, wire_msg: WireMsg) {
    match wire_msg {
        WireMsg::Handshake(h) => handle_rx_handshake(peer_addr, h),
        wire_msg => {
            ctx_mut(|c| {
                let conn = match c.connections.get_mut(&peer_addr) {
                    Some(conn) => conn,
                    None => {
                        trace!("Rxd wire-message from someone we don't know. Probably it was a \
                        pending stream when we dropped the peer connection. Ignoring this message \
                        from peer: {}", peer_addr);
                        return;
                    }
                };

                match conn.from_peer {
                    // TODO see if repetition can be reduced
                    FromPeer::NotNeeded => match conn.to_peer {
                        // Means we are a client
                        ToPeer::NoConnection | ToPeer::NotNeeded | ToPeer::Initiated { .. } => {
                            trace!(
                                "TODO Ignoring as we received something from some we are no \
                                 longer or not yet connected to"
                            );
                        }
                        ToPeer::Established {
                            ref q_conn,
                            ref peer_cert_der,
                        } => {
                            let node_info = NodeInfo {
                                peer_addr,
                                peer_cert_der: peer_cert_der.clone(),
                            };
                            dispatch_wire_msg(
                                node_info.into(),
                                q_conn,
                                c.our_ext_addr_tx.take(),
                                &c.event_tx,
                                wire_msg,
                                &mut c.bootstrap_cache,
                                conn.we_contacted_peer,
                            );
                        }
                    },
                    FromPeer::Established {
                        ref q_conn,
                        ref mut pending_reads,
                    } => match conn.to_peer {
                        ToPeer::NoConnection | ToPeer::Initiated { .. } => {
                            pending_reads.push(wire_msg);
                        }
                        ToPeer::NotNeeded => dispatch_wire_msg(
                            Peer::Client { peer_addr },
                            q_conn,
                            c.our_ext_addr_tx.take(),
                            &c.event_tx,
                            wire_msg,
                            &mut c.bootstrap_cache,
                            conn.we_contacted_peer,
                        ),
                        ToPeer::Established {
                            ref q_conn,
                            ref peer_cert_der,
                        } => {
                            let node_info = NodeInfo {
                                peer_addr,
                                peer_cert_der: peer_cert_der.clone(),
                            };
                            dispatch_wire_msg(
                                node_info.into(),
                                q_conn,
                                c.our_ext_addr_tx.take(),
                                &c.event_tx,
                                wire_msg,
                                &mut c.bootstrap_cache,
                                conn.we_contacted_peer,
                            );
                        }
                    },
                    FromPeer::NoConnection => unreachable!(
                        "Cannot have no connection for someone \
                         we got a message from"
                    ),
                }
            });
        }
    }
}

/// Dispatch wire message
// TODO: Improve by not taking `inform_tx` which is necessary right now to prevent double borrow
pub fn dispatch_wire_msg(
    peer: Peer,
    q_conn: &QConn,
    inform_tx: Option<Sender<SocketAddr>>,
    event_tx: &Sender<Event>,
    wire_msg: WireMsg,
    bootstrap_cache: &mut BootstrapCache,
    we_contacted_peer: bool,
) {
    match wire_msg {
        WireMsg::UserMsg(m) => {
            handle_user_msg(peer, event_tx, m, bootstrap_cache, we_contacted_peer)
        }
        WireMsg::EndpointEchoReq => handle_echo_req(peer.peer_addr(), q_conn),
        WireMsg::EndpointEchoResp(our_addr) => handle_echo_resp(our_addr, inform_tx),
        WireMsg::Handshake(_) => unreachable!("Should have been handled already"),
    }
}

fn handle_rx_handshake(peer_addr: SocketAddr, handshake: Handshake) {
    if let Handshake::Node { cert_der } = handshake {
        return handle_rx_cert(peer_addr, cert_der);
    }

    // Handshake from a client
    ctx_mut(|c| {
        let conn = match c.connections.get_mut(&peer_addr) {
            Some(conn) => conn,
            None => {
                trace!("Rxd handshake from someone we don't know. Probably it was a pending \
                stream when we dropped the peer connection. Ignoring this message from peer: {}",
                peer_addr);
                return;
            }
        };

        match conn.to_peer {
            ToPeer::NoConnection => (),
            ToPeer::NotNeeded | ToPeer::Initiated { .. } | ToPeer::Established { .. } => {
                // TODO consider booting this peer out
                debug!(
                    "Illegal handshake message - we have {:?} for the peer",
                    conn.to_peer
                );
                return;
            }
        }

        conn.to_peer = ToPeer::NotNeeded;

        let peer = Peer::Client { peer_addr };

        if let Err(e) = c.event_tx.send(Event::ConnectedTo { peer }) {
            info!("ERROR in informing user about a new peer: {:?} - {}", e, e);
        }
    })
}

fn handle_rx_cert(peer_addr: SocketAddr, peer_cert_der: Vec<u8>) {
    let node_info = NodeInfo {
        peer_addr,
        peer_cert_der,
    };

    let reverse_connect_to_peer = ctx_mut(|c| {
        // FIXME: Dropping the connection most probably will not drop the incoming stream
        // and then if you get a message on it you might still end up here without an entry
        // for the peer in your connection map. Fix by finding out the best way to drop the
        // incoming stream - probably use a select (on future) or something.
        //  NOTE: Even select might not help you if there are streams that are queued. The
        //  selector might select the stream before it selects the `terminator_leaf` so the
        //  actual fix needs to be done upstream
        let conn = match c.connections.get_mut(&peer_addr) {
            Some(conn) => conn,
            None => {
                trace!("Rxd certificate from someone we don't know. Probably it was a pending \
                stream when we dropped the peer connection. Ignoring this message from peer: {}",
                peer_addr);
                return false;
            }
        };

        match conn.to_peer {
            ToPeer::NoConnection => true,
            ToPeer::NotNeeded => {
                info!(
                    "TODO received a Node handshake from someone who has introduced oneself \
                     as a client before."
                );
                false
            }
            ToPeer::Initiated {
                ref peer_cert_der, ..
            }
            | ToPeer::Established {
                ref peer_cert_der, ..
            } => {
                if *peer_cert_der != node_info.peer_cert_der {
                    info!("TODO Certificate we have for the peer already doesn't match with \
                        the one given - we should disconnect to such peers - something fishy going \
                        on.");
                }
                false
            }
        }
    });

    if reverse_connect_to_peer {
        if let Err(e) = connect::connect_to(node_info, None, None) {
            debug!(
                "ERROR: Could not reverse connect to peer {}: {}",
                peer_addr, e
            );
        }
    }
}

fn handle_user_msg(
    peer: Peer,
    event_tx: &Sender<Event>,
    msg: bytes::Bytes,
    bootstrap_cache: &mut BootstrapCache,
    we_contacted_peer: bool,
) {
    let peer_addr = peer.peer_addr();
    let new_msg = Event::NewMessage { peer_addr, msg };
    if let Err(e) = event_tx.send(new_msg) {
        info!("Could not dispatch incoming user message: {:?}", e);
    }

    if let Peer::Node { node_info } = peer {
        if we_contacted_peer {
            bootstrap_cache.add_peer(node_info);
        }
    }
}

fn handle_echo_req(peer_addr: SocketAddr, q_conn: &QConn) {
    let msg = WireMsg::EndpointEchoResp(peer_addr);
    write_to_peer_connection(peer_addr, q_conn, msg);
}

fn handle_echo_resp(our_ext_addr: SocketAddr, inform_tx: Option<Sender<SocketAddr>>) {
    if let Some(tx) = inform_tx {
        if let Err(e) = tx.send(our_ext_addr) {
            info!("Error informing endpoint echo service response: {:?}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::testing::{rand_node_info, test_dirs};
    use std::collections::HashSet;
    use std::sync::mpsc;

    mod handle_user_msg {
        use super::*;

        #[test]
        fn when_peer_is_node_and_we_contacted_it_before_it_is_moved_to_bootstrap_cache_top() {
            let (event_tx, _event_rx) = mpsc::channel();
            let peer1 = rand_node_info();
            let peer2 = rand_node_info();
            let peer = Peer::Node {
                node_info: peer1.clone(),
            };
            let mut bootstrap_cache =
                unwrap!(BootstrapCache::try_new(&test_dirs(), HashSet::new()));
            bootstrap_cache.add_peer(peer1.clone());
            bootstrap_cache.add_peer(peer2.clone());

            handle_user_msg(
                peer,
                &event_tx,
                bytes::Bytes::from(vec![]),
                &mut bootstrap_cache,
                true,
            );

            let cached_peers: Vec<_> = bootstrap_cache.peers().iter().cloned().collect();
            assert_eq!(cached_peers, vec![peer2, peer1]);
        }
    }
}
