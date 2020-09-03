use quic_p2p::{QuicP2p, Config, send_msg, Message, read_bytes};
use std::{
    collections::HashSet,
    net::{IpAddr, Ipv4Addr, SocketAddr},
};
use bytes::Bytes;

/// Constructs a `QuicP2p` node with some sane defaults for testing.
fn new_quic_p2p() -> QuicP2p {
    new_quic_p2p_with_hcc(Default::default())
}

fn new_quic_p2p_with_hcc(
    hard_coded_contacts: HashSet<SocketAddr>,
) -> QuicP2p {
    QuicP2p::with_config(
        Some(Config {
            port: Some(0),
            ip: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
            hard_coded_contacts,
            ..Default::default()
        }),
        // Make sure we start with an empty cache. Otherwise, we might get into unexpected state.
        Default::default(),
        true,
    ).expect("Error creating QuicP2p object")
}

#[tokio::test]
async fn successful_connection() {
    let quic_p2p = new_quic_p2p();
    let peer1 = quic_p2p.new_endpoint().expect("Error creating endpoint 1");
    let peer1_addr = peer1.local_address();

    let peer2 = quic_p2p.new_endpoint().expect("Error creating endpoint 2");
    let _connection = peer2.connect_to(&peer1_addr).await.expect("Error creating connection between peers");

    let mut incoming_conn = peer1.listen().unwrap();
    let incoming_messages = incoming_conn.next().await.expect("Expected incoming connection");
    assert_eq!(incoming_messages.remote_addr(), peer2.local_address());
}

#[tokio::test]
async fn reusable_streams() {
    let quic_p2p = new_quic_p2p();
    let peer1 = quic_p2p.new_endpoint().expect("Error creating endpoint 1");
    let peer1_addr = peer1.local_address();

    let peer2 = quic_p2p.new_endpoint().expect("Error creating endpoint 2");
    let connection = peer2.connect_to(&peer1_addr).await.expect("Error creating connection between peers");

    let msg = Bytes::from(vec![1, 2, 3, 4]);
    let (mut send_stream, _recv_stream) = connection.send(msg.clone()).await.expect("Error sending message to peer");
    let mut incoming_conn = peer1.listen().unwrap();
    let mut incoming_messages = incoming_conn.next().await.expect("No incoming connection");
    let message = incoming_messages.next().await.expect("No incoming message");
    assert_eq!(msg, message.get_message_data());

    let mut recv_stream = if let Message::BiStream { recv, .. } = message {
            recv
    } else {
        panic!("Expected Bidirectional stream")
    };
    
    let msg = Bytes::from(vec![4, 3, 2, 1]);
    send_msg(&peer1_addr, &mut send_stream, msg.clone()).await.expect("Unable to send message");
    let recieved_message = read_bytes(&mut recv_stream.quinn_recv_stream, 0).await.unwrap();
    assert_eq!(msg, recieved_message);
}
