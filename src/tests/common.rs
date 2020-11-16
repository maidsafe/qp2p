use crate::{Config, Error, Message, QuicP2p, Result};
use bytes::Bytes;
use std::{
    collections::HashSet,
    net::{IpAddr, Ipv4Addr, SocketAddr},
};

/// Constructs a `QuicP2p` node with some sane defaults for testing.
pub fn new_qp2p() -> QuicP2p {
    new_qp2p_with_hcc(Default::default())
}

fn new_qp2p_with_hcc(hard_coded_contacts: HashSet<SocketAddr>) -> QuicP2p {
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

fn random_msg() -> Bytes {
    let random_bytes: Vec<u8> = (0..1024).map(|_| rand::random::<u8>()).collect();
    Bytes::from(random_bytes)
}

#[tokio::test]
async fn successful_connection() -> Result<()> {
    let qp2p = new_qp2p();
    let peer1 = qp2p.new_endpoint()?;
    let peer1_addr = peer1.socket_addr().await?;

    let peer2 = qp2p.new_endpoint()?;
    let _connection = peer2.connect_to(&peer1_addr).await?;

    let mut incoming_conn = peer1.listen();
    let incoming_messages = incoming_conn
        .next()
        .await
        .ok_or_else(|| Error::Unexpected("No incoming connection".to_string()))?;

    assert_eq!(incoming_messages.remote_addr(), peer2.socket_addr().await?);

    Ok(())
}

#[tokio::test]
async fn bi_directional_streams() -> Result<()> {
    let qp2p = new_qp2p();
    let peer1 = qp2p.new_endpoint()?;
    let peer1_addr = peer1.socket_addr().await?;

    let peer2 = qp2p.new_endpoint()?;
    let (connection, _) = peer2.connect_to(&peer1_addr).await?;

    let msg = random_msg();
    // Peer 2 sends a message and gets the bi-directional streams
    let (mut send_stream2, mut recv_stream2) = connection.send_bi(msg.clone()).await?;

    // Peer 1 gets an incoming connection
    let mut incoming_conn = peer1.listen();
    let mut incoming_messages = incoming_conn
        .next()
        .await
        .ok_or_else(|| Error::Unexpected("No incoming connection".to_string()))?;

    let message = incoming_messages
        .next()
        .await
        .ok_or_else(|| Error::Unexpected("No incoming message".to_string()))?;

    assert_eq!(msg, message.get_message_data());
    // Peer 1 gets the bi-directional streams along with the message
    let (mut recv_stream1, mut send_stream1) = if let Message::BiStream { recv, send, .. } = message
    {
        (recv, send)
    } else {
        return Err(Error::Unexpected(
            "Expected a Bidirectional stream".to_string(),
        ));
    };

    // Peer 2 should be able to re-use the stream to send an additional message
    let msg = random_msg();
    send_stream2.send_user_msg(msg.clone()).await?;

    // Peer 1 should recieve the message in the stream recieved along with the
    // previous message
    let recieved_message = recv_stream1.next().await?;
    assert_eq!(msg, recieved_message);

    // Peer 1 responds using the send stream
    let response_msg = random_msg();
    send_stream1.send_user_msg(response_msg.clone()).await?;

    let received_response = recv_stream2.next().await?;

    assert_eq!(response_msg, received_response);

    Ok(())
}

#[tokio::test]
async fn uni_directional_streams() -> Result<()> {
    let qp2p = new_qp2p();
    let peer1 = qp2p.new_endpoint()?;
    let peer1_addr = peer1.socket_addr().await?;
    let mut incoming_conn_peer1 = peer1.listen();

    let peer2 = qp2p.new_endpoint()?;
    let peer2_addr = peer2.socket_addr().await?;
    let mut incoming_conn_peer2 = peer2.listen();

    // Peer 2 sends a message
    let (conn_to_peer1, _) = peer2.connect_to(&peer1_addr).await?;
    let msg_from_peer2 = random_msg();
    conn_to_peer1.send_uni(msg_from_peer2.clone()).await?;
    drop(conn_to_peer1);

    // Peer 1 gets an incoming connection
    let mut incoming_messages = incoming_conn_peer1
        .next()
        .await
        .ok_or_else(|| Error::Unexpected("No incoming connection".to_string()))?;
    let message = incoming_messages
        .next()
        .await
        .ok_or_else(|| Error::Unexpected("No incoming message".to_string()))?;

    // Peer 1 gets the uni-directional stream along with the message
    let src = if let Message::UniStream { bytes, src, .. } = message {
        assert_eq!(msg_from_peer2, bytes);
        assert_eq!(src, peer2_addr);
        src
    } else {
        panic!("Expected a unidirectional stream")
    };

    // Peer 2 dropped the connection to peer 1 after sending the message, so the incoming message
    // stream gets closed. Drop the stream which also removes the connection from the connection
    // pool.
    assert!(incoming_messages.next().await.is_none());
    drop(incoming_messages);

    // Peer 1 sends back a message to Peer 2 on a new uni-directional stream
    let (conn_to_peer2, _) = peer1.connect_to(&src).await?;
    let msg_from_peer1 = random_msg();
    conn_to_peer2.send_uni(msg_from_peer1.clone()).await?;
    drop(conn_to_peer2);

    // Peer 2 should recieve the message
    let mut incoming_messages = incoming_conn_peer2
        .next()
        .await
        .ok_or_else(|| Error::Unexpected("No incoming connection".to_string()))?;
    let message = incoming_messages
        .next()
        .await
        .ok_or_else(|| Error::Unexpected("No incoming message".to_string()))?;

    // Peer 1 gets the uni-directional stream along with the message
    if let Message::UniStream { bytes, src, .. } = message {
        assert_eq!(msg_from_peer1, bytes);
        assert_eq!(src, peer1_addr);
    } else {
        panic!("Expected a unidirectional stream")
    }

    // Peer 1 dropped the connection to peer 2 after sending the message, so the incoming message
    // stream gets closed.
    assert!(incoming_messages.next().await.is_none());

    Ok(())
}

#[tokio::test]
async fn reuse_outgoing_connection() -> Result<()> {
    let qp2p = new_qp2p();
    let alice = qp2p.new_endpoint()?;

    let bob = qp2p.new_endpoint()?;
    let bob_addr = bob.socket_addr().await?;
    let mut bob_incoming_conns = bob.listen();

    // Connect for the first time and send a message.
    let (conn0, incoming_messages) = alice.connect_to(&bob_addr).await?;
    assert!(incoming_messages.is_some());
    let msg0 = random_msg();
    conn0.send_uni(msg0.clone()).await?;

    // Connect for the second time and send another message. This should reuse the previously
    // established connection.
    let (conn1, incoming_messages) = alice.connect_to(&bob_addr).await?;
    assert!(incoming_messages.is_none());
    let msg1 = random_msg();
    conn1.send_uni(msg1.clone()).await?;

    // Both messages should be received on the same stream.
    let mut incoming_messages = bob_incoming_conns
        .next()
        .await
        .ok_or_else(|| Error::Unexpected("no incoming connection".to_string()))?;

    assert_eq!(
        incoming_messages
            .next()
            .await
            .map(|msg| msg.get_message_data()),
        Some(msg0)
    );
    assert_eq!(
        incoming_messages
            .next()
            .await
            .map(|msg| msg.get_message_data()),
        Some(msg1)
    );

    Ok(())
}

#[tokio::test]
async fn reuse_incoming_connection() -> Result<()> {
    let qp2p = new_qp2p();
    let alice = qp2p.new_endpoint()?;
    let alice_addr = alice.socket_addr().await?;

    let bob = qp2p.new_endpoint()?;
    let bob_addr = bob.socket_addr().await?;
    let mut bob_incoming_conns = bob.listen();

    // Alice connects and sends a message.
    let (alice_conn, alice_incoming_messages) = alice.connect_to(&bob_addr).await?;
    let mut alice_incoming_messages = alice_incoming_messages.expect("no incoming messages");
    let msg0 = random_msg();
    alice_conn.send_uni(msg0.clone()).await?;

    // Bob receives incoming connection from alice
    let mut bob_incoming_messages0 = bob_incoming_conns
        .next()
        .await
        .ok_or_else(|| Error::Unexpected("no incoming connection".to_string()))?;
    assert_eq!(
        bob_incoming_messages0
            .next()
            .await
            .map(|msg| msg.get_message_data()),
        Some(msg0)
    );

    // Bob sends message back to Alice. This should reuse the same incoming connection.
    let (bob_conn, bob_incoming_messages1) = bob.connect_to(&alice_addr).await?;
    assert!(bob_incoming_messages1.is_none());

    let msg1 = random_msg();
    bob_conn.send_uni(msg1.clone()).await?;

    // Alice should receive Bob's message on the already established connection.
    assert_eq!(
        alice_incoming_messages
            .next()
            .await
            .map(|msg| msg.get_message_data()),
        Some(msg1)
    );

    Ok(())
}

#[tokio::test]
async fn remove_closed_connection_from_pool() -> Result<()> {
    let qp2p = new_qp2p();
    let alice = qp2p.new_endpoint()?;

    let bob = qp2p.new_endpoint()?;
    let bob_addr = bob.socket_addr().await?;
    let mut bob_incoming_conns = bob.listen();

    // Alice sends a message to Bob
    let (alice_conn, alice_incoming_messages) = alice.connect_to(&bob_addr).await?;
    let mut alice_incoming_messages = alice_incoming_messages.expect("no incoming messages");
    let msg0 = random_msg();
    alice_conn.send_uni(msg0.clone()).await?;

    // Bob receives the connection...
    let mut bob_incoming_messages = bob_incoming_conns
        .next()
        .await
        .ok_or_else(|| Error::Unexpected("no incoming connection".to_string()))?;
    assert_eq!(
        bob_incoming_messages
            .next()
            .await
            .map(|msg| msg.get_message_data()),
        Some(msg0)
    );
    // ..and closes the stream. This removes the connection from Bob's pool and because there are
    // no other live references to the connection it gets dropped...
    drop(bob_incoming_messages);

    // ...which closes it on Alice's side too.
    assert!(alice_incoming_messages.next().await.is_none());

    // Any attempt to send on the connection now fails.
    let msg1 = random_msg();
    assert!(alice_conn.send_uni(msg1).await.is_err());

    // Alice reconnects to Bob which creates new connection.
    let (alice_conn, alice_incoming_messages) = alice.connect_to(&bob_addr).await?;
    assert!(alice_incoming_messages.is_some());

    // Alice sends another message...
    let msg2 = random_msg();
    alice_conn.send_uni(msg2.clone()).await?;

    // ...which Bob receives on new connection.
    let mut bob_incoming_messages = bob_incoming_conns
        .next()
        .await
        .ok_or_else(|| Error::Unexpected("no incoming connection".to_string()))?;
    assert_eq!(
        bob_incoming_messages
            .next()
            .await
            .map(|msg| msg.get_message_data()),
        Some(msg2)
    );

    Ok(())
}
