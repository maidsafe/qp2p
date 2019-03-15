use crate::communicate::read_from_peer;
use crate::connect::connect_to;
use crate::context::{ctx_mut, Connection, ConnectionStatus};
use futures::Stream;
use std::collections::hash_map::Entry;
use tokio::runtime::current_thread;

/// Start listening
pub fn listen(incoming_connections: quinn::Incoming) {
    let leaf = incoming_connections.for_each(move |new_conn| {
        let connection = new_conn.connection;
        let incoming_streams = new_conn.incoming;

        let peer_addr = connection.remote_address();

        let mut establish_reverse_connection = false;
        let allow = ctx_mut(|c| match c.connections.entry(peer_addr) {
            Entry::Occupied(mut oe) => {
                let conn = oe.get_mut();
                if conn.from_peer.is_no_connection() {
                    conn.from_peer = ConnectionStatus::Established(connection);
                    establish_reverse_connection = conn.to_peer.status.is_no_connection();
                    true
                } else {
                    false
                }
            }
            Entry::Vacant(ve) => {
                let mut conn: Connection = Default::default();
                conn.from_peer = ConnectionStatus::Established(connection);
                ve.insert(conn);
                establish_reverse_connection = true;
                true
            }
        });

        if !allow {
            println!("Not allowing duplicate connection from peer: {}", peer_addr);
            return Ok(());
        }

        if establish_reverse_connection {
            if let Err(e) = connect_to(peer_addr) {
                println!(
                    "Dropping incoming connection as we could not reverse connect to peer {}: {:?}",
                    peer_addr, e
                );
                return Ok(());
            }
        }

        let leaf = incoming_streams
            .map_err(|e| println!("Connection closed due to: {:?}", e))
            .for_each(move |quic_stream| {
                read_from_peer(peer_addr, quic_stream);
                Ok(())
            });
        current_thread::spawn(leaf);
        Ok(())
    });
    current_thread::spawn(leaf);
}
