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

            // TODO make this a simple bool once the upstream bug is resolved and we don't need
            // this tx,rx workaround
            let is_not_duplicate = ctx_mut(|c| {
                let event_tx = c.event_tx.clone();
                let conn = c
                    .connections
                    .entry(peer_addr)
                    .or_insert_with(|| Connection::new(peer_addr, event_tx));
                let (tx, rx) = tokio::sync::oneshot::channel::<()>();
                let (tx_child, rx_child) = tokio::sync::watch::channel::<()>(());
                if conn.from_peer.is_no_connection() {
                    conn.from_peer = FromPeer::Established {
                        q_conn,
                        incoming_streams_terminator: Some(tx),
                        children_streams_terminator: tx_child,
                        pending_reads: Default::default(),
                    };
                    Some((rx, rx_child))
                // false
                } else {
                    None
                    // true
                }
            });

            // if is_duplicate {
            //     println!("Not allowing duplicate connection from peer: {}", peer_addr);
            //     return Ok(());
            // }
            let (terminator, rx_child) = match is_not_duplicate {
                Some(rxs) => rxs,
                None => {
                    println!("Not allowing duplicate connection from peer: {}", peer_addr);
                    return Ok(());
                }
            };

            let terminator_leaf = terminator
                .map_err(|e| println!("Incoming-streams terminator fired with error: {}", e));

            let leaf = incoming_streams
                .map_err(move |e| {
                    println!(
                        "TODO have a handler to destroy connection like in read/write/connect"
                    );
                    println!("Connection closed by peer {} due to: {:?}", peer_addr, e)
                })
                .for_each(move |quic_stream| {
                    if let Err(e) =
                        communicate::read_from_peer(peer_addr, quic_stream, rx_child.clone())
                    {
                        println!(
                            "Error reading from peer: {}. Closing all connections to it.",
                            e
                        );
                        // FIXME this should be the responsibility of the reader module
                        ctx_mut(|c| {
                            let _ = c.connections.remove(&peer_addr);
                        });
                        return Err(());
                    }
                    Ok(())
                })
                .select(terminator_leaf)
                .then(move |r| {
                    println!(
                        "TODO ==== Done from {} with is_err={}",
                        peer_addr,
                        r.is_err()
                    );
                    Ok(())
                });
            current_thread::spawn(leaf);
            Ok(())
        });
    current_thread::spawn(leaf);
}
