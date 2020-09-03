use bytes::Bytes;
use quic_p2p::{Config, Message, QuicP2p};
use std::{
    collections::HashSet,
    net::{IpAddr, Ipv4Addr, SocketAddr},
};

/// Constructs a `QuicP2p` node with some sane defaults for testing.
fn new_quic_p2p() -> QuicP2p {
    new_quic_p2p_with_hcc(Default::default())
}

fn new_quic_p2p_with_hcc(hard_coded_contacts: HashSet<SocketAddr>) -> QuicP2p {
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
    )
    .expect("Error creating QuicP2p object")
}

#[tokio::test]
async fn successful_connection() {
    let quic_p2p = new_quic_p2p();
    let peer1 = quic_p2p.new_endpoint().expect("Error creating endpoint 1");
    let peer1_addr = peer1.local_address();

    let peer2 = quic_p2p.new_endpoint().expect("Error creating endpoint 2");
    let _connection = peer2
        .connect_to(&peer1_addr)
        .await
        .expect("Error creating connection between peers");

    let mut incoming_conn = peer1.listen().unwrap();
    let incoming_messages = incoming_conn
        .next()
        .await
        .expect("Expected incoming connection");
    assert_eq!(incoming_messages.remote_addr(), peer2.local_address());
}

#[tokio::test]
async fn reusable_bidirectional_streams() {
    let quic_p2p = new_quic_p2p();
    let peer1 = quic_p2p.new_endpoint().expect("Error creating endpoint 1");
    let peer1_addr = peer1.local_address();

    let peer2 = quic_p2p.new_endpoint().expect("Error creating endpoint 2");
    let connection = peer2
        .connect_to(&peer1_addr)
        .await
        .expect("Error creating connection between peers");

    let msg = Bytes::from(vec![1, 2, 3, 4]);
    // Peer 2 sends a message and gets the bi-directional streams
    let (mut send_stream2, mut recv_stream2) = connection
        .send(msg.clone())
        .await
        .expect("Error sending message to peer");

    // Peer 1 gets an incoming connection
    let mut incoming_conn = peer1.listen().unwrap();
    let mut incoming_messages = incoming_conn.next().await.expect("No incoming connection");
    let message = incoming_messages.next().await.expect("No incoming message");
    assert_eq!(msg, message.get_message_data());
    // Peer 1 gets the bi-directional streams along with the message
    let (mut recv_stream1, mut send_stream1) = if let Message::BiStream { recv, send, .. } = message
    {
        (recv, send)
    } else {
        panic!("Expected Bidirectional stream")
    };

    // Peer 2 should be able to re-use the stream to send an additional message
    let msg = Bytes::from(vec![4, 3, 2, 1]);
    send_stream2
        .send(msg.clone())
        .await
        .expect("Unable to send message");

    // Peer 1 should recieve the message in the stream recieved along with the
    // previous message
    let recieved_message = recv_stream1.next().await.unwrap();
    assert_eq!(msg, recieved_message);

    // Peer 1 responds using the send stream
    let response_msg = Bytes::from(vec![5, 4, 3, 3]);
    send_stream1
        .send(response_msg.clone())
        .await
        .expect("Unable to send reponse via the stream");
    let received_response = recv_stream2
        .next()
        .await
        .expect("Unable to read response from the stream");
    assert_eq!(response_msg, received_response);
}
