use crate::communicate;
use crate::context::{ctx_mut, Connection, ConnectionStatus};
use crate::error::Error;
use crate::event::Event;
use crate::R;
use futures::Future;
use std::collections::hash_map::Entry;
use std::net::SocketAddr;
use std::{fmt, mem};
use tokio::runtime::current_thread;

/// Connect to the give peer
pub fn connect_to(peer_addr: SocketAddr) -> R<()> {
    let r = ctx_mut(|c| match c.connections.entry(peer_addr) {
        Entry::Occupied(mut oe) => {
            let conn = oe.get_mut();
            if conn.to_peer.status.is_no_connection() {
                conn.to_peer.status = ConnectionStatus::Initiated;
                c.quic_ep()
                    .connect(&peer_addr, "")
                    .map_err(Error::from)
                    .and_then(move |new_client_conn_fut| {
                        let leaf = new_client_conn_fut.then(move |new_client_conn_res| {
                            handle_new_connection_res(peer_addr, new_client_conn_res);
                            Ok(())
                        });

                        current_thread::spawn(leaf);
                        Ok(())
                    })
            } else {
                Err(Error::DuplicateConnectionToPeer(peer_addr))
            }
        }
        Entry::Vacant(ve) => {
            let mut conn: Connection = Default::default();
            conn.to_peer.status = ConnectionStatus::Initiated;
            ve.insert(conn);
            c.quic_ep()
                .connect(&peer_addr, "")
                .map_err(Error::from)
                .and_then(move |new_client_conn_fut| {
                    let leaf = new_client_conn_fut.then(move |new_client_conn_res| {
                        handle_new_connection_res(peer_addr, new_client_conn_res);
                        Ok(())
                    });

                    current_thread::spawn(leaf);
                    Ok(())
                })
        }
    });

    if let Err(e) = r.as_ref() {
        handle_connect_err(peer_addr, e);
    }

    r
}

fn handle_new_connection_res(
    peer_addr: SocketAddr,
    new_client_conn_res: Result<quinn::NewClientConnection, quinn::ConnectionError>,
) {
    match new_client_conn_res {
        Ok(new_client_conn) => {
            println!("Successfully connected to peer: {}", peer_addr);
            ctx_mut(|c| {
                let conn = unwrap!(
                    c.connections.get_mut(&peer_addr),
                    "Logic Error in bookkeeping - Report a bug!"
                );
                if conn.to_peer.status.is_initiated() {
                    panic!("Logic Error in book keeping - Report a BUG");
                }
                conn.to_peer.status = ConnectionStatus::Established(new_client_conn.connection);
                unwrap!(c.event_tx.send(Event::ConnectedTo { peer_addr }));

                let mut pending_sends =
                    mem::replace(&mut conn.to_peer.pending_sends, Default::default());
                match conn.to_peer.status {
                    ConnectionStatus::Established(ref conn) => {
                        while let Some(msg) = pending_sends.pop_front() {
                            communicate::write_to_peer_connection(peer_addr, conn, &msg);
                        }
                    }
                    ref x => unreachable!(
                        "Logic Error in status {:?} - we just got established \
                         above",
                        x
                    ),
                }
            })
        }
        Err(e) => handle_connect_err(peer_addr, e),
    }
}

fn handle_connect_err<E: fmt::Debug>(peer_addr: SocketAddr, err: E) {
    println!("Error connecting to peer {}: {:?}", peer_addr, err);
    ctx_mut(|c| {
        unwrap!(c.event_tx.send(Event::ConnectionFailure { peer_addr }));
        if let Entry::Occupied(oe) = c.connections.entry(peer_addr) {
            if !oe.get().from_peer.is_no_connection() {
                println!(
                    "Peer {} has a connection to us but we couldn't connect to it. \
                     All connections to this peer will now be severed.",
                    peer_addr
                );
            }
            oe.remove();
        }
    })
}
