use crate::communicate;
use crate::context::{ctx_mut, Connection, FromPeer};
use std::collections::hash_map::Entry;
use tokio::prelude::Future;
use tokio::prelude::Stream;
use tokio::runtime::current_thread;

/// Start listening
pub fn listen(incoming_connections: quinn::Incoming) {
    let leaf = incoming_connections.for_each(move |new_conn| {
        let q_conn = new_conn.connection;
        let incoming_streams = new_conn.incoming;

        let peer_addr = q_conn.remote_address();

        let is_duplicate = ctx_mut(|c| match c.connections.entry(peer_addr) {
            Entry::Occupied(mut oe) => {
                let conn = oe.get_mut();
                if conn.from_peer.is_no_connection() {
                    conn.from_peer = FromPeer::Established {
                        q_conn,
                        pending_reads: Default::default(),
                    };
                    true
                } else {
                    false
                }
            }
            Entry::Vacant(ve) => {
                let mut conn: Connection = Default::default();
                conn.from_peer = FromPeer::Established {
                    q_conn,
                    pending_reads: Default::default(),
                };
                ve.insert(conn);
                true
            }
        });

        if !is_duplicate {
            println!("Not allowing duplicate connection from peer: {}", peer_addr);
            return Ok(());
        }

        let leaf = incoming_streams
            .map_err(move |e| println!("Connection closed by peer {} due to: {:?}", peer_addr, e))
            .for_each(move |quic_stream| {
                communicate::read_from_peer(peer_addr, quic_stream);
                Ok(())
            })
            .then(|r| {
                println!("TODO ==== Done with is_err={}", r.is_err());
                r
            });
        current_thread::spawn(leaf);
        Ok(())
    });
    current_thread::spawn(leaf);
}
