use crate::communicate;
use crate::context::{ctx_mut, Connection, FromPeer};
use tokio::prelude::{Future, Stream};
use tokio::runtime::current_thread;

/// Start listening
pub fn listen(incoming_connections: quinn::Incoming) {
    let leaf = incoming_connections
        .map_err(|e| println!("ERROR: Listener errored out: {:?}", e))
        .for_each(move |new_conn| {
            let q_conn = new_conn.connection;
            let incoming_streams = new_conn.incoming;

            let peer_addr = q_conn.remote_address();

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
                    false
                } else {
                    true
                }
            });

            if is_duplicate {
                println!("Not allowing duplicate connection from peer: {}", peer_addr);
                return Ok(());
            }

            let leaf = incoming_streams
                .map_err(move |e| {
                    println!("Connection closed by peer {} due to: {:?}", peer_addr, e)
                })
                .for_each(move |quic_stream| {
                    if let Err(e) = communicate::read_from_peer(peer_addr, quic_stream) {
                        println!(
                            "Error \"{}\" reading from peer. Closing all connections to it.",
                            e
                        );
                        ctx_mut(|c| {
                            let _ = c.connections.remove(&peer_addr);
                        });
                        return Err(());
                    }
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
