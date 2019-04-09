use crate::context::{ctx, ctx_mut, Connection, FromPeer, ToPeer};
use crate::error::Error;
use crate::event::Event;
use crate::wire_msg::WireMsg;
use crate::R;
use crate::{connect, CrustInfo};
use std::net::SocketAddr;
use std::sync::mpsc::Sender;
use tokio::prelude::Future;
use tokio::runtime::current_thread;

/// Send message to peer. If the peer is not connected, it will attempt to connect to it first
/// and then send the message
pub fn try_write_to_peer(peer_info: CrustInfo, msg: WireMsg) {
    let connect_and_send = ctx_mut(|c| {
        let peer_addr = peer_info.peer_addr;
        let event_tx = c.event_tx.clone();
        let conn = c
            .connections
            .entry(peer_addr)
            .or_insert_with(|| Connection::new(peer_addr, event_tx));
        match conn.to_peer {
            ToPeer::NoConnection => Some(msg),
            ToPeer::Initiated {
                ref peer_cert_der,
                ref mut pending_sends,
            } => {
                if *peer_cert_der != peer_info.peer_cert_der {
                    println!("TODO Certificate we have for the peer already doesn't match with the \
                    one given - we should disconnect to such peers - something fishy going on.");
                }
                pending_sends.push(msg);
                None
            }
            ToPeer::Established { ref q_conn, .. } => {
                write_to_peer_connection(peer_info.peer_addr, q_conn, msg);
                None
            }
        }
    });

    if connect_and_send.is_some() {
        let peer_addr = peer_info.peer_addr;
        if let Err(e) = connect::connect_to(peer_info, connect_and_send) {
            println!(
                "Unable to connect to peer {} to be able to send message: {:?}",
                peer_addr, e
            );
        }
    }
}

/// This will fail if we don't have a connection to the peer or if the peer is in an invalid state
/// to be sent a message to.
#[allow(unused)]
pub fn write_to_peer(peer_addr: SocketAddr, msg: WireMsg) {
    ctx(|c| {
        let conn = match c.connections.get(&peer_addr) {
            Some(conn) => conn,
            None => return println!("Asked to communicate with an unknown peer: {}", peer_addr),
        };

        let q_conn = match &conn.to_peer {
            ToPeer::Established { ref q_conn, .. } => q_conn,
            x => {
                return println!(
                    "Peer {} is in invalid state {:?} to be communicated to",
                    peer_addr, x
                );
            }
        };

        write_to_peer_connection(peer_addr, q_conn, msg);
    })
}

/// Write to the peer, given the QUIC connection to it
pub fn write_to_peer_connection(
    peer_addr: SocketAddr,
    conn: &quinn::Connection,
    wire_msg: WireMsg,
) {
    let leaf = conn
        .open_uni()
        .map_err(move |e| {
            handle_communication_err(peer_addr, &From::from(e), "Open-Unidirectional")
        })
        .and_then(move |o_stream| {
            tokio::io::write_all(o_stream, wire_msg.into())
                .map_err(move |e| handle_communication_err(peer_addr, &From::from(e), "Write-All"))
        })
        .and_then(move |(o_stream, _): (_, bytes::Bytes)| {
            tokio::io::shutdown(o_stream).map_err(move |e| {
                handle_communication_err(peer_addr, &From::from(e), "Shutdown-after-write")
            })
        })
        .map(|_| ());

    current_thread::spawn(leaf);
}

/// Read messages from peer
pub fn read_from_peer(peer_addr: SocketAddr, quic_stream: quinn::NewStream) -> R<()> {
    let i_stream = match quic_stream {
        quinn::NewStream::Bi(_bi) => {
            let e = Error::BiDirectionalStreamAttempted(peer_addr);
            handle_communication_err(peer_addr, &e, "Receiving Stream");
            return Err(e);
        }
        quinn::NewStream::Uni(uni) => uni,
    };

    let leaf = quinn::read_to_end(i_stream, ctx(|c| c.max_msg_size_allowed))
        .map_err(move |e| handle_communication_err(peer_addr, &From::from(e), "Read-To-End"))
        .and_then(move |(_i_stream, raw)| {
            WireMsg::from_raw(raw.into())
                .map_err(|e| handle_communication_err(peer_addr, &e, "Raw to WireMsg"))
                .map(|wire_msg| handle_wire_msg(peer_addr, wire_msg))
        });

    current_thread::spawn(leaf);

    Ok(())
}

/// Handle wire messages from peer
pub fn handle_wire_msg(peer_addr: SocketAddr, wire_msg: WireMsg) {
    match wire_msg {
        WireMsg::CertificateDer(cert) => handle_rx_cert(peer_addr, cert),
        wire_msg => {
            ctx_mut(|c| {
                let conn = match c.connections.get_mut(&peer_addr) {
                    Some(conn) => conn,
                    None => {
                        println!("Rxd wire-message from someone we don't know. Probably it was a \
                        pending stream when we dropped the peer connection. Ignoring this message \
                        from peer: {}", peer_addr);
                        return;
                    }
                };

                match conn.from_peer {
                    FromPeer::Established {
                        ref mut pending_reads,
                        ..
                    } => match conn.to_peer {
                        ToPeer::NoConnection | ToPeer::Initiated { .. } => {
                            pending_reads.push(wire_msg);
                        }
                        ToPeer::Established { ref q_conn, .. } => dispatch_wire_msg(
                            peer_addr,
                            q_conn,
                            c.our_ext_addr_tx.take(),
                            &c.event_tx,
                            wire_msg,
                        ),
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
    peer_addr: SocketAddr,
    q_conn: &quinn::Connection,
    inform_tx: Option<Sender<SocketAddr>>,
    event_tx: &Sender<Event>,
    wire_msg: WireMsg,
) {
    match wire_msg {
        WireMsg::UserMsg(m) => handle_user_msg(peer_addr, event_tx, m),
        WireMsg::EndpointEchoReq => handle_echo_req(peer_addr, q_conn),
        WireMsg::EndpointEchoResp(our_addr) => handle_echo_resp(our_addr, inform_tx),
        WireMsg::CertificateDer(_) => unreachable!("Should have been handled already"),
    }
}

fn handle_rx_cert(peer_addr: SocketAddr, peer_cert_der: Vec<u8>) {
    let peer_info = CrustInfo {
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
                println!("Rxd certificate from someone we don't know. Probably it was a pending \
                stream when we dropped the peer connection. Ignoring this message from peer: {}",
                peer_addr);
                return false;
            }
        };

        match conn.to_peer {
            ToPeer::NoConnection => true,
            ToPeer::Initiated {
                ref peer_cert_der, ..
            }
            | ToPeer::Established {
                ref peer_cert_der, ..
            } => {
                if *peer_cert_der != peer_info.peer_cert_der {
                    println!("TODO Certificate we have for the peer already doesn't match with \
                        the one given - we should disconnect to such peers - something fishy going \
                        on.");
                }
                false
            }
        }
    });

    if reverse_connect_to_peer {
        if let Err(e) = connect::connect_to(peer_info, None) {
            println!(
                "ERROR: Could not reverse connect to peer {}: {}",
                peer_addr, e
            );
        }
    }
}

fn handle_user_msg(peer_addr: SocketAddr, event_tx: &Sender<Event>, msg: bytes::Bytes) {
    let new_msg = Event::NewMessage { peer_addr, msg };
    if let Err(e) = event_tx.send(new_msg) {
        println!("Could not dispatch incoming user message: {:?}", e);
    }
}

fn handle_echo_req(peer_addr: SocketAddr, q_conn: &quinn::Connection) {
    let msg = WireMsg::EndpointEchoResp(peer_addr);
    write_to_peer_connection(peer_addr, q_conn, msg);
}

fn handle_echo_resp(our_ext_addr: SocketAddr, inform_tx: Option<Sender<SocketAddr>>) {
    if let Some(tx) = inform_tx {
        if let Err(e) = tx.send(our_ext_addr) {
            println!("Error informing endpoint echo service response: {:?}", e);
        }
    }
}

/// When connection fails, remove it to prevent memory leaks.
fn handle_communication_err(peer_addr: SocketAddr, e: &Error, details: &str) {
    println!(
        "ERROR in communication with peer {}: {:?} - {}. Details: {}",
        peer_addr, e, e, details
    );
    let _ = ctx_mut(|c| c.connections.remove(&peer_addr));
}
