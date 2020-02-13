// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::bootstrap_cache::BootstrapCache;
use crate::connection::{Connection, FromPeer, QConn, ToPeer};
use crate::context::{ctx, ctx_mut};
use crate::error::QuicP2pError;
use crate::event::Event;
use crate::utils::{self, Token};
use crate::wire_msg::{Handshake, WireMsg};
use crate::{connect, EventSenders};
use crate::{Peer, R};
use bytes::Bytes;
use futures::future::TryFutureExt;
use futures::stream::StreamExt;
use log::{debug, info, trace, warn};
use std::{io, net::SocketAddr, sync::mpsc};

/// Send message to peer. If the peer is a node and is not connected, it will attempt to connect to
/// it first and then send the message. For un-connected clients, it'll simply error out.
pub fn try_write_to_peer(peer: Peer, msg: WireMsg, token: Token) -> R<()> {
    let node_addr = match peer {
        Peer::Client(peer_addr) => {
            let user_msg = if let WireMsg::UserMsg(ref msg) = msg {
                Some((Peer::Client(peer_addr), msg.clone(), token))
            } else {
                None
            };
            if let Err(e) = write_to_peer(peer, msg, token) {
                utils::handle_communication_err(
                    peer_addr,
                    &e,
                    "Sending to a Client Peer",
                    user_msg,
                );
            }
            return Ok(());
        }
        Peer::Node(peer_addr) => peer_addr,
    };

    let connect_and_send = {
        ctx_mut(|c| {
            let event_tx = c.event_tx.clone();
            let conn = c
                .connections
                .entry(node_addr)
                .or_insert_with(|| Connection::new(node_addr, event_tx, None));

            match conn.to_peer {
                ToPeer::NoConnection => Some((msg, token)),
                ToPeer::NotNeeded => {
                    warn!("TODO We normally can't get here - ignoring");
                    None
                }
                ToPeer::Initiated {
                    ref mut pending_sends,
                    ..
                } => {
                    pending_sends.push((msg, token));
                    None
                }
                ToPeer::Established { ref q_conn, .. } => {
                    write_to_peer_connection(Peer::Node(node_addr), q_conn, msg, token);
                    None
                }
            }
        })
    };

    if connect_and_send.is_some() {
        connect::connect_to(node_addr, connect_and_send, None)?;
    }

    Ok(())
}

/// This will fail if we don't have a connection to the peer or if the peer is in an invalid state
/// to be sent a message to.
pub fn write_to_peer(peer: Peer, msg: WireMsg, token: Token) -> R<()> {
    ctx(|c| {
        let conn = match c.connections.get(&peer.peer_addr()) {
            Some(conn) => conn,
            None => {
                trace!(
                    "Asked to communicate with an unknown peer: {}",
                    peer.peer_addr()
                );
                return Err(QuicP2pError::Io(io::Error::new(
                    io::ErrorKind::Other,
                    "Unknown Peer",
                )));
            }
        };

        match &conn.to_peer {
            ToPeer::NotNeeded => {
                if let FromPeer::Established { ref q_conn, .. } = conn.from_peer {
                    write_to_peer_connection(peer, q_conn, msg, token);
                } else {
                    return Err(QuicP2pError::Io(io::Error::new(
                        io::ErrorKind::Other,
                        &format!(
                            "We cannot communicate with someone we are not needing to connect to \
                             and they are not connected to us just now. Peer: {}",
                            peer.peer_addr()
                        )[..],
                    )));
                }
            }
            ToPeer::Established { ref q_conn, .. } => {
                write_to_peer_connection(peer, q_conn, msg, token)
            }
            ToPeer::NoConnection | ToPeer::Initiated { .. } => {
                return Err(QuicP2pError::Io(io::Error::new(
                    io::ErrorKind::Other,
                    &format!(
                        "Peer {} is in invalid state {:?} to be communicated to",
                        peer.peer_addr(),
                        conn.to_peer
                    )[..],
                )));
            }
        }

        Ok(())
    })
}

/// Write to the peer, given the QUIC connection to it
pub fn write_to_peer_connection(peer: Peer, conn: &QConn, wire_msg: WireMsg, token: Token) {
    let peer_addr = peer.peer_addr();
    let user_msg = if let WireMsg::UserMsg(ref m) = wire_msg {
        Some((peer, m.clone(), token))
    } else {
        None
    };

    let uni_stream = conn.open_uni();

    let leaf = async move {
        let mut o_stream = match uni_stream.await {
            Ok(o_stream) => o_stream,
            Err(e) => {
                utils::handle_communication_err(
                    peer_addr,
                    &From::from(e),
                    "Open-Unidirectional",
                    user_msg,
                );
                return;
            }
        };

        let (message, msg_flag) = wire_msg.into();

        if let Err(e) = o_stream.write_all(&message[..]).await {
            utils::handle_communication_err(peer_addr, &From::from(e), "Write-All", user_msg);
            return;
        }
        if let Err(e) = o_stream.write_all(&[msg_flag]).await {
            utils::handle_communication_err(peer_addr, &From::from(e), "Write-All", user_msg);
            return;
        }

        if let Err(e) = o_stream.finish().await {
            utils::handle_communication_err(
                peer_addr,
                &From::from(e),
                "Shutdown-after-write",
                user_msg,
            );
            return;
        }

        utils::handle_send_success(user_msg);
    };

    let _ = tokio::spawn(leaf);
}

/// Listen for incoming streams containing peer messages and read them when available
pub fn read_from_peer(
    peer_addr: SocketAddr,
    mut uni_streams: quinn::IncomingUniStreams,
    mut bi_streams: quinn::IncomingBiStreams,
) {
    let _ = tokio::spawn(async move {
        if let Some(res) = bi_streams.next().await {
            let err = match res {
                Err(e) => {
                    debug!(
                        "Error in Incoming-bi-stream while reading from peer {}: {:?} - {}.\nNote: It
                         would not be allowed even if it didn't fail as bi-streams are not allowed",
                        peer_addr, e, e
                    );
                    From::from(e)
                }
                Ok((_o_stream, _i_stream)) => {
                    let e = QuicP2pError::BiDirectionalStreamAttempted { peer_addr };
                    debug!(
                        "Error in Incoming-streams while reading from peer {}: {:?} - {}.",
                        peer_addr, e, e
                    );
                    e
                }
            };
            utils::handle_communication_err(peer_addr, &err, "Receiving Stream", None);
        }
    });

    let _ = tokio::spawn(async move {
        while let Some(res) = uni_streams.next().await {
            match res {
                Err(e) => {
                    utils::handle_communication_err(
                        peer_addr,
                        &From::from(e),
                        "Incoming streams failed",
                        None,
                    );
                }
                Ok(i_stream) => read_peer_stream(peer_addr, i_stream),
            }
        }
    });
}

fn read_peer_stream(peer_addr: SocketAddr, i_stream: quinn::RecvStream) {
    let leaf = i_stream
        .read_to_end(ctx(|c| c.max_msg_size_allowed))
        .map_err(move |e| {
            utils::handle_communication_err(peer_addr, &From::from(e), "Read-To-End", None)
        })
        .map_ok(move |raw| {
            WireMsg::from_raw(raw)
                .map_err(|e| utils::handle_communication_err(peer_addr, &e, "Raw to WireMsg", None))
                .map(|wire_msg| handle_wire_msg(peer_addr, wire_msg))
        });

    let _ = tokio::spawn(leaf);
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
                        trace!(
                            "Rxd wire-message from someone we don't know. Probably it was a \
                        pending stream when we dropped the peer connection. Ignoring this message \
                        from peer: {}",
                            peer_addr
                        );
                        return;
                    }
                };

                match conn.from_peer {
                    // TODO see if repetition can be reduced
                    FromPeer::NotNeeded => match conn.to_peer {
                        // Means we are a client
                        ToPeer::NoConnection | ToPeer::NotNeeded | ToPeer::Initiated { .. } => {
                            trace!(
                                "TODO Ignoring as we received something from someone we are no \
                                 longer or not yet connected to"
                            );
                        }
                        ToPeer::Established { ref q_conn } => {
                            dispatch_wire_msg(
                                Peer::Node(peer_addr),
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
                            Peer::Client(peer_addr),
                            q_conn,
                            c.our_ext_addr_tx.take(),
                            &c.event_tx,
                            wire_msg,
                            &mut c.bootstrap_cache,
                            conn.we_contacted_peer,
                        ),
                        ToPeer::Established { ref q_conn } => {
                            dispatch_wire_msg(
                                Peer::Node(peer_addr),
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
    inform_tx: Option<mpsc::Sender<SocketAddr>>,
    event_tx: &EventSenders,
    wire_msg: WireMsg,
    bootstrap_cache: &mut BootstrapCache,
    we_contacted_peer: bool,
) {
    match wire_msg {
        WireMsg::UserMsg(m) => {
            handle_user_msg(peer, event_tx, m, bootstrap_cache, we_contacted_peer)
        }
        WireMsg::EndpointEchoReq => handle_echo_req(peer, q_conn),
        WireMsg::EndpointEchoResp(our_addr) => handle_echo_resp(our_addr, inform_tx),
        WireMsg::Handshake(_) => unreachable!("Should have been handled already"),
    }
}

fn handle_rx_handshake(peer_addr: SocketAddr, handshake: Handshake) {
    if let Handshake::Node = handshake {
        return handle_rx_handshake_from_node(peer_addr);
    }

    // Handshake from a client
    ctx_mut(|c| {
        let conn = match c.connections.get_mut(&peer_addr) {
            Some(conn) => conn,
            None => {
                trace!(
                    "Rxd handshake from someone we don't know. Probably it was a pending \
                stream when we dropped the peer connection. Ignoring this message from peer: {}",
                    peer_addr
                );
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

        let peer = Peer::Client(peer_addr);

        if let Err(e) = c.event_tx.send(Event::ConnectedTo { peer }) {
            info!("ERROR in informing user about a new peer: {:?} - {}", e, e);
        }
    })
}

fn handle_rx_handshake_from_node(peer_addr: SocketAddr) {
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
                trace!(
                    "Rxd certificate from someone we don't know. Probably it was a pending \
                     stream when we dropped the peer connection. Ignoring this message from peer: {}",
                    peer_addr
                );
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
            ToPeer::Initiated { .. } | ToPeer::Established { .. } => false,
        }
    });

    if reverse_connect_to_peer {
        if let Err(e) = connect::connect_to(peer_addr, None, None) {
            debug!(
                "ERROR: Could not reverse connect to peer {}: {}",
                peer_addr, e
            );
        }
    }
}

fn handle_user_msg(
    peer: Peer,
    event_tx: &EventSenders,
    msg: Bytes,
    bootstrap_cache: &mut BootstrapCache,
    we_contacted_peer: bool,
) {
    let new_msg = Event::NewMessage {
        peer: peer.clone(),
        msg,
    };
    if let Err(e) = event_tx.send(new_msg) {
        info!("Could not dispatch incoming user message: {:?}", e);
    }

    if let Peer::Node(node_addr) = peer {
        if we_contacted_peer {
            bootstrap_cache.add_peer(node_addr);
        }
    }
}

fn handle_echo_req(peer: Peer, q_conn: &QConn) {
    let msg = WireMsg::EndpointEchoResp(peer.peer_addr());
    write_to_peer_connection(peer, q_conn, msg, 0);
}

fn handle_echo_resp(our_ext_addr: SocketAddr, inform_tx: Option<mpsc::Sender<SocketAddr>>) {
    if let Some(tx) = inform_tx {
        if let Err(e) = tx.send(our_ext_addr) {
            info!("Error informing endpoint echo service response: {:?}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{
        new_random_qp2p, new_unbounded_channels, rand_node_addr, test_dirs, write_to_bi_stream,
    };
    use std::collections::HashSet;
    use unwrap::unwrap;

    // Test for the case of bi-directional stream usage attempt.
    #[test]
    fn disallow_bidirectional_streams() {
        let (mut qp2p0, rx0) = new_random_qp2p(false, Default::default());
        let qp2p0_info = unwrap!(qp2p0.our_connection_info());

        let (mut qp2p1, rx1) = {
            let mut hcc: HashSet<_> = Default::default();
            assert!(hcc.insert(qp2p0_info.clone()));
            new_random_qp2p(true, hcc)
        };
        let qp2p1_info = unwrap!(qp2p1.our_connection_info());

        // Drain the message queues
        while let Ok(_) = rx1.try_recv() {}
        while let Ok(_) = rx0.try_recv() {}

        // Create a bi-directional stream and write data
        qp2p1.el.post(move || {
            write_to_bi_stream(qp2p0_info, WireMsg::UserMsg(From::from("123")));
        });

        // The connection should fail because we don't allow bi-directional streams
        match rx0.recv() {
            Ok(Event::ConnectionFailure { peer, err }) => {
                assert_eq!(peer.peer_addr(), qp2p1_info);
                assert_eq!(
                    format!("{}", err),
                    format!("{}", QuicP2pError::ConnectionCancelled)
                );
            }
            r => panic!("Unexpected result {:?}", r),
        }
    }

    mod handle_user_msg {
        use super::*;

        #[test]
        fn when_peer_is_node_and_we_contacted_it_before_it_is_moved_to_bootstrap_cache_top() {
            let (event_tx, _event_rx) = new_unbounded_channels();
            let peer1_addr = rand_node_addr();
            let peer2_addr = rand_node_addr();
            let peer = Peer::Node(peer1_addr);
            let mut bootstrap_cache =
                unwrap!(BootstrapCache::new(Default::default(), Some(&test_dirs())));
            bootstrap_cache.add_peer(peer1_addr);
            bootstrap_cache.add_peer(peer2_addr);

            handle_user_msg(
                peer,
                &event_tx,
                bytes::Bytes::from(vec![]),
                &mut bootstrap_cache,
                true,
            );

            let cached_peers: Vec<_> = bootstrap_cache.peers().iter().cloned().collect();
            assert_eq!(cached_peers, vec![peer2_addr, peer1_addr]);
        }
    }
}
