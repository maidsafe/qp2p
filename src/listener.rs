use crate::communicate;
use crate::context::{ctx_mut, Connection, FromPeer, ToPeer};
use crate::event::Event;
use crate::CrustInfo;
use tokio::prelude::{Future, Stream};
use tokio::runtime::current_thread;

/// Start listening
pub fn listen(incoming_connections: quinn::Incoming) {
    let leaf = incoming_connections
        .map_err(|()| println!("ERROR: Listener errored out"))
        .for_each(move |new_conn| {
            handle_new_conn(new_conn);
            Ok(())
        });

    current_thread::spawn(leaf);
}

fn handle_new_conn(new_conn: quinn::NewConnection) {
    // FIXME: If we don't have a connection to the peer yet, don't wait indefinitely for them to
    // send in their certificate - have a delay and then log and forget them
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
        let (tx, rx) = tokio::sync::oneshot::channel();
        let (tx_child, rx_child) = tokio::sync::watch::channel(());
        if conn.from_peer.is_no_connection() {
            conn.from_peer = FromPeer::Established {
                q_conn,
                incoming_streams_terminator: tx,
                children_streams_terminator: tx_child,
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
            return;
        }
    };

    let terminator_leaf =
        terminator.map_err(|e| println!("Incoming-streams terminator fired with error: {}", e));

    let leaf = incoming_streams
        .map_err(move |e| {
            println!(
                "Incoming-streams from peer {} closed due to: {:?} - {}",
                peer_addr, e, e
            );
            let _ = ctx_mut(|c| c.connections.remove(&peer_addr));
        })
        .for_each(move |quic_stream| {
            communicate::read_from_peer(peer_addr, quic_stream, rx_child.clone()).map_err(|e| {
                println!(
                    "Error in Incoming-streams while reading from peer {}: {:?} - {}.",
                    peer_addr, e, e
                )
            })
        })
        .select(terminator_leaf)
        .then(move |_| Ok(()));

    current_thread::spawn(leaf);
}
