use crate::event::Event;
use crate::event_loop::EventLoopState;
use crate::wire_msg::WireMsg;
use futures::{Future, Stream};
use std::net::SocketAddr;
use std::sync::mpsc::Sender;
use tokio::runtime::current_thread;

/// Start listening
pub fn listen(el_state: EventLoopState, incoming_connections: quinn::Incoming) {
    let leaf = incoming_connections.for_each(move |new_conn| {
        let connection = new_conn.connection;
        let incoming_streams = new_conn.incoming;

        let el_state = el_state.clone();
        let peer_addr = connection.remote_address();

        let leaf = incoming_streams
            .map_err(|e| println!("Connection closed due to: {:?}", e))
            .for_each(move |quic_stream| {
                handle_peer_req(el_state.clone(), quic_stream, peer_addr);
                Ok(())
            });
        current_thread::spawn(leaf);
        Ok(())
    });
    current_thread::spawn(leaf);
}

fn handle_peer_req(el_state: EventLoopState, quic_stream: quinn::NewStream, peer: SocketAddr) {
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
            let event_tx = el_state.tx();
            let wire_msg = unwrap!(bincode::deserialize(&*raw));
            match wire_msg {
                WireMsg::UserMsg(msg) => {
                    let new_msg = Event::NewMessage { peer, msg };
                    unwrap!(event_tx.send(new_msg));
                }
                x => panic!("No handler for message: {:?}", x),
            }
            Ok(())
        });

    current_thread::spawn(leaf);
}
