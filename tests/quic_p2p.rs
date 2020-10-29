use bytes::Bytes;
use qp2p::{Config, Error, Message, QuicP2p, Result};
use std::{
    collections::HashSet,
    net::{IpAddr, Ipv4Addr, SocketAddr},
};

/// Constructs a `QuicP2p` node with some sane defaults for testing.
fn new_qp2p() -> QuicP2p {
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
    let peer1_addr = peer1.our_addr()?;

    let peer2 = qp2p.new_endpoint()?;
    let _connection = peer2.connect_to(&peer1_addr).await?;

    let mut incoming_conn = peer1.listen()?;
    let incoming_messages = incoming_conn
        .next()
        .await
        .ok_or_else(|| Error::Unexpected("No incoming connection".to_string()))?;

    assert_eq!(incoming_messages.remote_addr(), peer2.our_addr()?);

    Ok(())
}

#[tokio::test]
async fn bi_directional_streams() -> Result<()> {
    let qp2p = new_qp2p();
    let peer1 = qp2p.new_endpoint()?;
    let peer1_addr = peer1.our_addr()?;

    let peer2 = qp2p.new_endpoint()?;
    let connection = peer2.connect_to(&peer1_addr).await?;

    let msg = random_msg();
    // Peer 2 sends a message and gets the bi-directional streams
    let (mut send_stream2, mut recv_stream2) = connection.send(msg.clone()).await?;

    // Peer 1 gets an incoming connection
    let mut incoming_conn = peer1.listen()?;
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
    send_stream2.send(msg.clone()).await?;

    // Peer 1 should recieve the message in the stream recieved along with the
    // previous message
    let recieved_message = recv_stream1.next().await?;
    assert_eq!(msg, recieved_message);

    // Peer 1 responds using the send stream
    let response_msg = random_msg();
    send_stream1.send(response_msg.clone()).await?;

    let received_response = recv_stream2.next().await?;

    assert_eq!(response_msg, received_response);

    Ok(())
}

#[tokio::test]
async fn uni_directional_streams() -> Result<()> {
    let qp2p = new_qp2p();
    let peer1 = qp2p.new_endpoint()?;
    let peer1_addr = peer1.our_addr()?;
    let mut incoming_conn_peer1 = peer1.listen()?;

    let peer2 = qp2p.new_endpoint()?;
    let peer2_addr = peer2.our_addr()?;
    let mut incoming_conn_peer2 = peer2.listen()?;

    // Peer 2 sends a message
    let conn_to_peer1 = peer2.connect_to(&peer1_addr).await?;

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
        return Err(Error::UnexpectedMessageType);
    };

    // Peer 1 sends back a message to Peer 2 on a new uni-directional stream
    let msg_from_peer1 = random_msg();
    let conn_to_peer2 = peer1.connect_to(&src).await?;
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
        Ok(())
    } else {
        Err(Error::Unexpected(
            "Expected a Unidirectional stream".to_string(),
        ))
    }
}
