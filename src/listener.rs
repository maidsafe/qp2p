use crate::communicate;
use crate::context::{ctx_mut, Connection, FromPeer, ToPeer};
use crate::error::Error;
use crate::event::Event;
use crate::CrustInfo;
use std::net::SocketAddr;
use tokio::prelude::{Future, Stream};
use tokio::runtime::current_thread;

/// Start listening
pub fn listen(incoming_connections: quinn::Incoming) {
    let leaf = incoming_connections
        .map_err(|()| println!("ERROR: Listener errored out"))
        .for_each(move |(conn_driver, q_conn, incoming)| {
            handle_new_conn(conn_driver, q_conn, incoming);
            Ok(())
        });

    current_thread::spawn(leaf);
}

fn handle_new_conn(
    conn_driver: quinn::ConnectionDriver,
    q_conn: quinn::Connection,
    incoming_streams: quinn::IncomingStreams,
) {
    // FIXME: If we don't have a connection to the peer yet, don't wait indefinitely for them to
    // send in their certificate - have a delay and then log and forget them
    let peer_addr = q_conn.remote_address();

    current_thread::spawn(conn_driver.map_err(move |e| {
        handle_connection_err(
            peer_addr,
            &From::from(e),
            "Driver failed",
        );
    }));

    let is_duplicate = ctx_mut(|c| {
        let event_tx = c.event_tx.clone();
        let conn = c
            .connections
            .entry(peer_addr)
            .or_insert_with(|| Connection::new(peer_addr, event_tx));
        if conn.from_peer.is_no_connection() {
            conn.from_peer = FromPeer::Established {
                q_conn,
                pending_reads: Default::default(),
            };

            if let ToPeer::Established {
                ref peer_cert_der, ..
            } = conn.to_peer
            {
                let crust_info = CrustInfo {
                    peer_addr,
                    peer_cert_der: peer_cert_der.clone(),
                };

                if let Err(e) = c.event_tx.send(Event::ConnectedTo { crust_info }) {
                    println!("ERROR in informing user about a new peer: {:?} - {}", e, e);
                }
            }
            None
        } else {
            Some(q_conn)
        }
    });

    if let Some(q_conn) = is_duplicate {
        println!("Not allowing duplicate connection from peer: {}", peer_addr);
        q_conn.close(0, &[]);
        return;
    }

    let leaf = incoming_streams
        .map_err(move |e| {
            handle_connection_err(peer_addr, &From::from(e), "Incoming streams failed");
        })
        .for_each(move |quic_stream| {
            communicate::read_from_peer(peer_addr, quic_stream).map_err(|e| {
                println!(
                    "Error in Incoming-streams while reading from peer {}: {:?} - {}.",
                    peer_addr, e, e
                )
            })
        });

    current_thread::spawn(leaf);
}

/// Removes failed connection from connection list to prevent from any type of memory leaks.
fn handle_connection_err(peer_addr: SocketAddr, e: &Error, details: &str) {
    println!(
        "Incoming connection with peer {} errored: {:?} - {}. Details: {}",
        peer_addr, e, e, details
    );
    let _ = ctx_mut(|c| c.connections.remove(&peer_addr));
}
