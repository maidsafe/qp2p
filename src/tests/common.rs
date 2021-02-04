use super::{new_qp2p, random_msg};
use crate::utils;
use anyhow::{anyhow, Result};
use futures::future;
use std::time::Duration;
use tokio::time::timeout;

#[tokio::test]
async fn successful_connection() -> Result<()> {
    utils::init_logging();

    let qp2p = new_qp2p()?;
    let (peer1, mut peer1_incoming_connections, _, _) = qp2p.new_endpoint().await?;
    let peer1_addr = peer1.socket_addr();

    let (peer2, _, _, _) = qp2p.new_endpoint().await?;
    peer2.connect_to(&peer1_addr).await?;
    let peer2_addr = peer2.socket_addr();

    if let Some(connecting_peer) = peer1_incoming_connections.next().await {
        assert_eq!(connecting_peer, peer2_addr);
    } else {
        anyhow!("No incoming connection");
    }

    Ok(())
}

#[tokio::test]
async fn single_message() -> Result<()> {
    utils::init_logging();

    let qp2p = new_qp2p()?;
    let (peer1, mut peer1_incoming_connections, mut peer1_incoming_messages, _) =
        qp2p.new_endpoint().await?;
    let peer1_addr = peer1.socket_addr();

    let (peer2, _, _, _) = qp2p.new_endpoint().await?;
    let peer2_addr = peer2.socket_addr();

    // Peer 2 connects and sends a message
    peer2.connect_to(&peer1_addr).await?;
    let msg_from_peer2 = random_msg();
    peer2
        .send_message(msg_from_peer2.clone(), &peer1_addr)
        .await?;

    // Peer 1 gets an incoming connection
    if let Some(connecting_peer) = peer1_incoming_connections.next().await {
        assert_eq!(connecting_peer, peer2_addr);
    } else {
        anyhow!("No incoming connection");
    }

    // Peer 2 gets an incoming message
    if let Some((source, message)) = peer1_incoming_messages.next().await {
        assert_eq!(source, peer2_addr);
        assert_eq!(message, msg_from_peer2);
    } else {
        anyhow!("No incoming message");
    }
    Ok(())
}

#[tokio::test]
async fn reuse_outgoing_connection() -> Result<()> {
    utils::init_logging();

    let qp2p = new_qp2p()?;
    let (alice, _, _, _) = qp2p.new_endpoint().await?;
    let alice_addr = alice.socket_addr();

    let (bob, mut bob_incoming_connections, mut bob_incoming_messages, _) =
        qp2p.new_endpoint().await?;
    let bob_addr = bob.socket_addr();

    // Connect for the first time and send a message.
    alice.connect_to(&bob_addr).await?;
    let msg0 = random_msg();
    alice.send_message(msg0.clone(), &bob_addr).await?;

    // Bob should recieve an incoming connection and message
    if let Some(connecting_peer) = bob_incoming_connections.next().await {
        assert_eq!(connecting_peer, alice_addr);
    } else {
        anyhow!("No incoming connection");
    }

    if let Some((source, message)) = bob_incoming_messages.next().await {
        assert_eq!(source, alice_addr);
        assert_eq!(message, msg0);
    } else {
        anyhow!("No incoming message");
    }

    // Try connecting again and send a message
    alice.connect_to(&bob_addr).await?;
    let msg1 = random_msg();
    alice.send_message(msg1.clone(), &bob_addr).await?;

    // Bob *should not* get an incoming connection since there is already a connection established
    if let Ok(Some(connecting_peer)) =
        timeout(Duration::from_secs(2), bob_incoming_connections.next()).await
    {
        anyhow!("Unexpected incoming connection from {}", connecting_peer);
    }

    if let Some((source, message)) = bob_incoming_messages.next().await {
        assert_eq!(source, alice_addr);
        assert_eq!(message, msg1);
    } else {
        anyhow!("No incoming message");
    }
    Ok(())
}

#[tokio::test]
async fn reuse_incoming_connection() -> Result<()> {
    utils::init_logging();

    let qp2p = new_qp2p()?;
    let (alice, mut alice_incoming_connections, mut alice_incoming_messages, _) =
        qp2p.new_endpoint().await?;
    let alice_addr = alice.socket_addr();

    let (bob, mut bob_incoming_connections, mut bob_incoming_messages, _) =
        qp2p.new_endpoint().await?;
    let bob_addr = bob.socket_addr();

    // Connect for the first time and send a message.
    alice.connect_to(&bob_addr).await?;
    let msg0 = random_msg();
    alice.send_message(msg0.clone(), &bob_addr).await?;

    // Bob should recieve an incoming connection and message
    if let Some(connecting_peer) = bob_incoming_connections.next().await {
        assert_eq!(connecting_peer, alice_addr);
    } else {
        anyhow!("No incoming connection");
    }

    if let Some((source, message)) = bob_incoming_messages.next().await {
        assert_eq!(source, alice_addr);
        assert_eq!(message, msg0);
    } else {
        anyhow!("No incoming message");
    }

    // Bob tries to connect to alice and sends a message
    bob.connect_to(&alice_addr).await?;
    let msg1 = random_msg();
    bob.send_message(msg1.clone(), &alice_addr).await?;

    // Alice *will not* get an incoming connection since there is already a connection established
    // However, Alice will still get the incoming message
    if let Ok(Some(connecting_peer)) =
        timeout(Duration::from_secs(2), alice_incoming_connections.next()).await
    {
        anyhow!("Unexpected incoming connection from {}", connecting_peer);
    }

    if let Some((source, message)) = alice_incoming_messages.next().await {
        assert_eq!(source, bob_addr);
        assert_eq!(message, msg1);
    } else {
        anyhow!("No incoming message");
    }
    Ok(())
}

#[tokio::test]
async fn disconnection() -> Result<()> {
    utils::init_logging();

    let qp2p = new_qp2p()?;
    let (mut alice, mut alice_incoming_connections, _, mut alice_disconnections) =
        qp2p.new_endpoint().await?;
    let alice_addr = alice.socket_addr();

    let (bob, mut bob_incoming_connections, _, mut bob_disconnections) =
        qp2p.new_endpoint().await?;
    let bob_addr = bob.socket_addr();

    // Alice connects to Bob who should receive an incoming connection.
    alice.connect_to(&bob_addr).await?;

    if let Some(connecting_peer) = bob_incoming_connections.next().await {
        assert_eq!(connecting_peer, alice_addr);
    } else {
        anyhow!("No incoming connection");
    }

    // After Alice disconnects from Bob both peers should receive the disconnected event.
    alice.disconnect_from(&bob_addr)?;

    if let Some(disconnected_peer) = alice_disconnections.next().await {
        assert_eq!(disconnected_peer, bob_addr);
    } else {
        anyhow!("Missing disconnection event");
    }

    if let Some(disconnected_peer) = bob_disconnections.next().await {
        assert_eq!(disconnected_peer, alice_addr);
    } else {
        anyhow!("Missing disconnection event");
    }

    // This time bob connects to Alice. Since this is a *new connection*, Alice should get the connection event
    bob.connect_to(&alice_addr).await?;

    if let Some(connected_peer) = alice_incoming_connections.next().await {
        assert_eq!(connected_peer, bob_addr);
    } else {
        anyhow!("Missing incoming connection");
    }

    Ok(())
}

#[tokio::test]
async fn simultaneous_incoming_and_outgoing_connections() -> Result<()> {
    // If both peers call `connect_to` simultaneously (that is, before any of them receives the
    // others connection first), two separate connections are created. This test verifies that
    // everything still works correctly even in this case.

    utils::init_logging();

    let qp2p = new_qp2p()?;
    let (
        alice,
        mut alice_incoming_connections,
        mut alice_incoming_messages,
        mut alice_disconnections,
    ) = qp2p.new_endpoint().await?;
    let alice_addr = alice.socket_addr();

    let (mut bob, mut bob_incoming_connections, mut bob_incoming_messages, _) =
        qp2p.new_endpoint().await?;
    let bob_addr = bob.socket_addr();

    future::try_join(alice.connect_to(&bob_addr), bob.connect_to(&alice_addr)).await?;

    if let Some(connecting_peer) = alice_incoming_connections.next().await {
        assert_eq!(connecting_peer, bob_addr);
    } else {
        anyhow!("No incoming connection from Bob");
    }

    if let Some(connecting_peer) = bob_incoming_connections.next().await {
        assert_eq!(connecting_peer, alice_addr);
    } else {
        anyhow!("No incoming connectino from Alice");
    }

    let msg0 = random_msg();
    alice.send_message(msg0.clone(), &bob_addr).await?;

    let msg1 = random_msg();
    bob.send_message(msg1.clone(), &alice_addr).await?;

    if let Some((src, message)) = alice_incoming_messages.next().await {
        assert_eq!(src, bob_addr);
        assert_eq!(message, msg1);
    } else {
        anyhow!("Missing incoming message");
    }

    if let Some((src, message)) = bob_incoming_messages.next().await {
        assert_eq!(src, alice_addr);
        assert_eq!(message, msg0);
    } else {
        anyhow!("Missing incoming message");
    }

    // Drop the connection initiated by Bob.
    bob.disconnect_from(&alice_addr)?;

    // It should be closed on Alice's side too.
    if let Some(disconnected_peer) = alice_disconnections.next().await {
        assert_eq!(disconnected_peer, bob_addr);
    } else {
        anyhow!("Missing disconnection event");
    }

    // Bob connects to Alice again. This does not open a new connection but returns the connection
    // previously initiated by Alice from the pool.
    bob.connect_to(&alice_addr).await?;

    if let Ok(Some(connecting_peer)) =
        timeout(Duration::from_secs(2), alice_incoming_connections.next()).await
    {
        anyhow!("Unexpected incoming connection from {}", connecting_peer);
    }

    let msg2 = random_msg();
    bob.send_message(msg2.clone(), &alice_addr).await?;

    if let Some((src, message)) = alice_incoming_messages.next().await {
        assert_eq!(src, bob_addr);
        assert_eq!(message, msg2);
    }

    Ok(())
}

#[tokio::test]
async fn multiple_concurrent_connects_to_the_same_peer() -> Result<()> {
    utils::init_logging();

    let qp2p = new_qp2p()?;
    let (alice, mut alice_incoming_connections, mut alice_incoming_messages, _) =
        qp2p.new_endpoint().await?;
    let alice_addr = alice.socket_addr();

    let (bob, _, mut bob_incoming_messages, _) = qp2p.new_endpoint().await?;
    let bob_addr = bob.socket_addr();

    // Try to establish two connections to the same peer at the same time.
    let ((), ()) =
        future::try_join(bob.connect_to(&alice_addr), bob.connect_to(&alice_addr)).await?;

    // Alice get only one incoming connection
    if let Some(connecting_peer) = alice_incoming_connections.next().await {
        assert_eq!(connecting_peer, bob_addr);
    } else {
        anyhow!("Missing incoming connection");
    }

    if let Ok(Some(connecting_peer)) =
        timeout(Duration::from_secs(2), alice_incoming_connections.next()).await
    {
        anyhow!("Unexpected incoming connection from {}", connecting_peer);
    }

    // Send two messages, one from each end
    let msg0 = random_msg();
    alice.send_message(msg0.clone(), &bob_addr).await?;

    let msg1 = random_msg();
    bob.send_message(msg1.clone(), &alice_addr).await?;

    // Both messages are received  at the other end
    if let Some((src, message)) = alice_incoming_messages.next().await {
        assert_eq!(src, bob_addr);
        assert_eq!(message, msg1);
    } else {
        anyhow!("No message received from Bob");
    }

    if let Some((src, message)) = bob_incoming_messages.next().await {
        assert_eq!(src, alice_addr);
        assert_eq!(message, msg0);
    } else {
        anyhow!("No message from alice");
    }

    Ok(())
}
