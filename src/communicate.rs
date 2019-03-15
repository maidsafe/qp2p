use crate::connect;
use crate::context::{ctx, ctx_mut, Connection, ConnectionStatus};
use crate::event::Event;
use crate::wire_msg::WireMsg;
use futures::Future;
use std::collections::hash_map::Entry;
use std::net::SocketAddr;
use tokio::runtime::current_thread;

/// Send message to peer. If the peer is not connected, it will attempt to connect to it first
/// and then send the message
pub fn try_write_to_peer(peer_addr: SocketAddr, msg: WireMsg) {
    let should_connect = ctx_mut(|c| match c.connections.entry(peer_addr) {
        Entry::Occupied(mut oe) => match oe.get().to_peer.status {
            ConnectionStatus::Initiated => {
                oe.get_mut().to_peer.pending_sends.push_back(msg);
                false
            }
            ConnectionStatus::Established(ref conn) => {
                write_to_peer_connection(peer_addr, conn, &msg);
                false
            }
            ConnectionStatus::NoConnection => {
                oe.get_mut().to_peer.pending_sends.push_back(msg);
                true
            }
        },
        Entry::Vacant(ve) => {
            let mut conn: Connection = Default::default();
            conn.to_peer.pending_sends.push_back(msg);
            ve.insert(conn);
            true
        }
    });

    if should_connect {
        if let Err(e) = connect::connect_to(peer_addr) {
            println!(
                "Unable to connect to peer {} to be able to send message: {:?}",
                peer_addr, e
            );
        }
    }
}

#[allow(unused)]
pub fn write_to_peer(peer_addr: SocketAddr, msg: &WireMsg) {
    ctx(|c| {
        let conn = match c.connections.get(&peer_addr) {
            Some(conn) => conn,
            None => return println!("Asked to communicate with an unknown peer: {}", peer_addr),
        };

        let conn = match &conn.to_peer.status {
            ConnectionStatus::Established(ref conn) => conn,
            x => {
                return println!(
                    "Peer {} is in invalid state {:?} to be communicated to",
                    peer_addr, x
                );
            }
        };

        write_to_peer_connection(peer_addr, conn, msg);
    })
}

pub fn write_to_peer_connection(
    peer_addr: SocketAddr,
    conn: &quinn::Connection,
    wire_msg: &WireMsg,
) {
    let m = unwrap!(bincode::serialize(wire_msg));
    let leaf = conn
        .open_uni() // TODO How do you close the stream
        .map_err(move |e| println!("Error opening write stream to peer {}: {:?}", peer_addr, e))
        .map(move |o_stream| tokio::io::write_all(o_stream, m))
        .then(move |r| {
            if let Err(e) = r {
                println!("Error writing to peer {}; {:?}", peer_addr, e)
            }
            Ok(())
        });

    current_thread::spawn(leaf);
}

pub fn read_from_peer(peer_addr: SocketAddr, quic_stream: quinn::NewStream) {
    let i_stream = match quic_stream {
        quinn::NewStream::Bi(_bi) => {
            print!("No code handling for bi-directional stream");
            return;
        }
        quinn::NewStream::Uni(uni) => uni,
    };

    let leaf = quinn::read_to_end(i_stream, 64 * 1024)
        .map_err(|e| println!("Error reading stream: {:?}", e))
        .and_then(move |(_i_stream, raw)| {
            let wire_msg = unwrap!(bincode::deserialize(&*raw));
            match wire_msg {
                WireMsg::UserMsg(m) => handle_user_msg(peer_addr, m),
                WireMsg::EndpointEchoReq => handle_echo_req(peer_addr),
                WireMsg::EndpointEchoResp(our_addr) => handle_echo_resp(our_addr),
            }
            Ok(())
        });

    current_thread::spawn(leaf);
}

fn handle_user_msg(peer_addr: SocketAddr, msg: Vec<u8>) {
    let new_msg = Event::NewMessage { peer_addr, msg };
    ctx(|c| unwrap!(c.event_tx.send(new_msg)));
}

fn handle_echo_req(peer_addr: SocketAddr) {
    let msg = WireMsg::EndpointEchoResp(peer_addr);
    try_write_to_peer(peer_addr, msg);
}

fn handle_echo_resp(our_ext_addr: SocketAddr) {
    ctx_mut(|c| {
        if let Some(tx) = c.our_ext_addr_tx.take() {
            if let Err(e) = tx.send(our_ext_addr) {
                println!("Error informing endpoint echo service response: {:?}", e);
            }
        }
    })
}
