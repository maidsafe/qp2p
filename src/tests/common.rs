// Copyright 2021 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use super::{hash, local_addr, new_endpoint, random_msg};
use crate::{Config, Endpoint, RetryConfig};
use color_eyre::eyre::{bail, eyre, Report, Result};
use futures::{future, stream::FuturesUnordered, StreamExt};
use std::{collections::BTreeSet, time::Duration};
use tokio::time::timeout;
use tracing::info;
use tracing_test::traced_test;

#[tokio::test(flavor = "multi_thread")]
async fn successful_connection() -> Result<()> {
    let (peer1, mut peer1_incoming_connections, _, _, _) = new_endpoint().await?;
    let peer1_addr = peer1.public_addr();

    let (peer2, _, _, _, _) = new_endpoint().await?;
    peer2.connect_to(&peer1_addr).await.map(drop)?;
    let peer2_addr = peer2.public_addr();

    if let Some(connecting_peer) = peer1_incoming_connections.next().await {
        assert_eq!(connecting_peer, peer2_addr);
    } else {
        eyre!("No incoming connection");
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn single_message() -> Result<()> {
    let (peer1, mut peer1_incoming_connections, mut peer1_incoming_messages, _, _) =
        new_endpoint().await?;
    let peer1_addr = peer1.public_addr();

    let (peer2, _, _, _, _) = new_endpoint().await?;
    let peer2_addr = peer2.public_addr();

    // Peer 2 connects and sends a message
    let p2_to_1 = peer2.connect_to(&peer1_addr).await?;
    let msg_from_peer2 = random_msg(1024);
    p2_to_1.send_uni(msg_from_peer2.clone(), 0).await?;

    // Peer 1 gets an incoming connection
    if let Some(connecting_peer) = peer1_incoming_connections.next().await {
        assert_eq!(connecting_peer, peer2_addr);
    } else {
        eyre!("No incoming connection");
    }

    // Peer 2 gets an incoming message
    if let Some((source, message)) = peer1_incoming_messages.next().await {
        assert_eq!(source.remote_address(), peer2_addr);
        assert_eq!(message, msg_from_peer2);
    } else {
        eyre!("No incoming message");
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn reuse_outgoing_connection() -> Result<()> {
    let (alice, _, _, _, _) = new_endpoint().await?;
    let alice_addr = alice.public_addr();

    let (bob, mut bob_incoming_connections, mut bob_incoming_messages, _, _) =
        new_endpoint().await?;
    let bob_addr = bob.public_addr();

    // Connect for the first time and send a message.
    let a_to_b = alice.connect_to(&bob_addr).await?;
    let msg0 = random_msg(1024);
    a_to_b.send_uni(msg0.clone(), 0).await?;

    // Bob should recieve an incoming connection and message
    if let Some(connecting_peer) = bob_incoming_connections.next().await {
        assert_eq!(connecting_peer, alice_addr);
    } else {
        eyre!("No incoming connection");
    }

    if let Some((source, message)) = bob_incoming_messages.next().await {
        assert_eq!(source.remote_address(), alice_addr);
        assert_eq!(message, msg0);
    } else {
        eyre!("No incoming message");
    }

    // Send a message on the same connection
    let a_to_b = alice.connect_to(&bob_addr).await?;
    let msg1 = random_msg(1024);
    a_to_b.send_uni(msg1.clone(), 0).await?;

    // Bob *should not* get an incoming connection since there is already a connection established
    if let Ok(Some(connecting_peer)) =
        timeout(Duration::from_secs(2), bob_incoming_connections.next()).await
    {
        eyre!("Unexpected incoming connection from {}", connecting_peer);
    }

    if let Some((source, message)) = bob_incoming_messages.next().await {
        assert_eq!(source.remote_address(), alice_addr);
        assert_eq!(message, msg1);
    } else {
        eyre!("No incoming message");
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn reuse_incoming_connection() -> Result<()> {
    let (alice, mut alice_incoming_connections, mut alice_incoming_messages, _, _) =
        new_endpoint().await?;
    let alice_addr = alice.public_addr();

    let (bob, mut bob_incoming_connections, mut bob_incoming_messages, _, _) =
        new_endpoint().await?;
    let bob_addr = bob.public_addr();

    // Connect for the first time and send a message.
    let a_to_b = alice.connect_to(&bob_addr).await?;
    let msg0 = random_msg(1024);
    a_to_b.send_uni(msg0.clone(), 0).await?;

    // Bob should recieve an incoming connection and message
    if let Some(connecting_peer) = bob_incoming_connections.next().await {
        assert_eq!(connecting_peer, alice_addr);
    } else {
        eyre!("No incoming connection");
    }

    if let Some((source, message)) = bob_incoming_messages.next().await {
        assert_eq!(source.remote_address(), alice_addr);
        assert_eq!(message, msg0);
    } else {
        eyre!("No incoming message");
    }

    // Bob tries to connect to alice and sends a message
    let b_to_a = bob.connect_to(&alice_addr).await?;
    let msg1 = random_msg(1024);
    b_to_a.send_uni(msg1.clone(), 0).await?;

    // Alice *will not* get an incoming connection since there is already a connection established
    // However, Alice will still get the incoming message
    if let Ok(Some(connecting_peer)) =
        timeout(Duration::from_secs(2), alice_incoming_connections.next()).await
    {
        eyre!("Unexpected incoming connection from {}", connecting_peer);
    }

    if let Some((source, message)) = alice_incoming_messages.next().await {
        assert_eq!(source.remote_address(), bob_addr);
        assert_eq!(message, msg1);
    } else {
        eyre!("No incoming message");
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn disconnection() -> Result<()> {
    let (alice, mut alice_incoming_connections, _, mut alice_disconnections, _) =
        new_endpoint().await?;
    let alice_addr = alice.public_addr();

    let (bob, mut bob_incoming_connections, _, mut bob_disconnections, _) = new_endpoint().await?;
    let bob_addr = bob.public_addr();

    // Alice connects to Bob who should receive an incoming connection.
    let a_to_b = alice.connect_to(&bob_addr).await?;

    if let Some(connecting_peer) = bob_incoming_connections.next().await {
        assert_eq!(connecting_peer, alice_addr);
    } else {
        eyre!("No incoming connection");
    }

    // After Alice disconnects from Bob both peers should receive the disconnected event.
    drop(a_to_b);

    if let Some(disconnected_peer) = alice_disconnections.next().await {
        assert_eq!(disconnected_peer, bob_addr);
    } else {
        eyre!("Missing disconnection event");
    }

    if let Some(disconnected_peer) = bob_disconnections.next().await {
        assert_eq!(disconnected_peer, alice_addr);
    } else {
        eyre!("Missing disconnection event");
    }

    // This time bob connects to Alice. Since this is a *new connection*, Alice should get the connection event
    bob.connect_to(&alice_addr).await.map(drop)?;

    if let Some(connected_peer) = alice_incoming_connections.next().await {
        assert_eq!(connected_peer, bob_addr);
    } else {
        eyre!("Missing incoming connection");
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn simultaneous_incoming_and_outgoing_connections() -> Result<()> {
    // If both peers call `connect_to` simultaneously (that is, before any of them receives the
    // others connection first), two separate connections are created. This test verifies that
    // everything still works correctly even in this case.

    let (
        alice,
        mut alice_incoming_connections,
        mut alice_incoming_messages,
        mut alice_disconnections,
        _,
    ) = new_endpoint().await?;
    let alice_addr = alice.public_addr();

    let (bob, mut bob_incoming_connections, mut bob_incoming_messages, _, _) =
        new_endpoint().await?;
    let bob_addr = bob.public_addr();

    let (a_to_b, b_to_a) =
        future::try_join(alice.connect_to(&bob_addr), bob.connect_to(&alice_addr)).await?;

    if let Some(connecting_peer) = alice_incoming_connections.next().await {
        assert_eq!(connecting_peer, bob_addr);
    } else {
        eyre!("No incoming connection from Bob");
    }

    if let Some(connecting_peer) = bob_incoming_connections.next().await {
        assert_eq!(connecting_peer, alice_addr);
    } else {
        eyre!("No incoming connectino from Alice");
    }

    let msg0 = random_msg(1024);
    a_to_b.send_uni(msg0.clone(), 0).await?;

    let msg1 = random_msg(1024);
    b_to_a.send_uni(msg1.clone(), 0).await?;

    if let Some((src, message)) = alice_incoming_messages.next().await {
        assert_eq!(src.remote_address(), bob_addr);
        assert_eq!(message, msg1);
    } else {
        eyre!("Missing incoming message");
    }

    if let Some((src, message)) = bob_incoming_messages.next().await {
        assert_eq!(src.remote_address(), alice_addr);
        assert_eq!(message, msg0);
    } else {
        eyre!("Missing incoming message");
    }

    // Drop the connection initiated by Bob.
    drop(b_to_a);

    // It should be closed on Alice's side too.
    if let Some(disconnected_peer) = alice_disconnections.next().await {
        assert_eq!(disconnected_peer, bob_addr);
    } else {
        eyre!("Missing disconnection event");
    }

    // Bob connects to Alice again. This does not open a new connection but returns the connection
    // previously initiated by Alice from the pool.
    let b_to_a = bob.connect_to(&alice_addr).await?;

    if let Ok(Some(connecting_peer)) =
        timeout(Duration::from_secs(2), alice_incoming_connections.next()).await
    {
        eyre!("Unexpected incoming connection from {}", connecting_peer);
    }

    let msg2 = random_msg(1024);
    b_to_a.send_uni(msg2.clone(), 0).await?;

    if let Some((src, message)) = alice_incoming_messages.next().await {
        assert_eq!(src.remote_address(), bob_addr);
        assert_eq!(message, msg2);
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn multiple_connections_with_many_concurrent_messages() -> Result<()> {
    use futures::future;

    let num_senders: usize = 10;
    let num_messages_each: usize = 100;
    let num_messages_total: usize = 1000;

    let (server_endpoint, _, mut recv_incoming_messages, _, _) = new_endpoint().await?;
    let server_addr = server_endpoint.public_addr();

    let test_msgs: Vec<_> = (0..num_messages_each).map(|_| random_msg(1024)).collect();
    let sending_msgs = test_msgs.clone();

    let mut tasks = Vec::new();

    // Receiver
    tasks.push(tokio::spawn({
        async move {
            let mut num_received = 0;
            let mut sending_tasks = Vec::new();

            while let Some((src, msg)) = recv_incoming_messages.next().await {
                info!(
                    "received from {:?} with message size {}",
                    src.remote_address(),
                    msg.len()
                );
                assert_eq!(msg.len(), test_msgs[0].len());

                sending_tasks.push(tokio::spawn({
                    async move {
                        // Hash the inputs for couple times to simulate certain workload.
                        let hash_result = hash(&msg);
                        for _ in 0..5 {
                            let _ = hash(&msg);
                        }
                        // Send the hash result back.
                        src.send_uni(hash_result.to_vec().into(), 0).await?;

                        Ok::<_, Report>(())
                    }
                }));

                num_received += 1;
                if num_received >= num_messages_total {
                    break;
                }
            }

            let _ = future::try_join_all(sending_tasks).await?;

            Ok(())
        }
    }));

    // Sender
    for id in 0..num_senders {
        let messages = sending_msgs.clone();
        tasks.push(tokio::spawn({
            let (send_endpoint, _, mut recv_incoming_messages, _, _) = new_endpoint().await?;

            async move {
                let mut hash_results = BTreeSet::new();
                let conn = send_endpoint.connect_to(&server_addr).await?;
                for (index, message) in messages.iter().enumerate().take(num_messages_each) {
                    let _ = hash_results.insert(hash(message));
                    info!("sender #{} sending message #{}", id, index);
                    conn.send_uni(message.clone(), 0).await?;
                }

                info!(
                    "sender #{} completed sending messages, starts listening",
                    id
                );

                while let Some((src, msg)) = recv_incoming_messages.next().await {
                    info!(
                        "#{} received from server {:?} with message size {}",
                        id,
                        src.remote_address(),
                        msg.len()
                    );

                    info!("Hash len before: {:?}", hash_results.len());
                    assert!(hash_results.remove(&msg[..]));
                    info!("Hash len after: {:?}", hash_results.len());

                    if hash_results.is_empty() {
                        break;
                    }
                }

                Ok::<_, Report>(())
            }
        }));
    }

    let _ = future::try_join_all(tasks).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
#[traced_test]
async fn multiple_connections_with_many_larger_concurrent_messages() -> Result<()> {
    let num_senders: usize = 10;
    let num_messages_each: usize = 100;
    let num_messages_total: usize = num_senders * num_messages_each;

    let (server_endpoint, _, mut recv_incoming_messages, _, _) = new_endpoint().await?;
    let server_addr = server_endpoint.public_addr();

    let test_msgs: Vec<_> = (0..num_messages_each)
        .map(|_| random_msg(1024 * 1024))
        .collect();
    let sending_msgs = test_msgs.clone();

    let mut tasks = FuturesUnordered::new();

    // Receiver + Hasher
    tasks.push(tokio::spawn({
        async move {
            let mut num_received = 0;
            assert!(!logs_contain("error"));

            while let Some((src, msg)) = recv_incoming_messages.next().await {
                info!(
                    "received from {:?} with message size {}",
                    src.remote_address(),
                    msg.len()
                );
                assert!(!logs_contain("error"));

                assert_eq!(msg.len(), test_msgs[0].len());

                let hash_result = hash(&msg);
                for _ in 0..5 {
                    let _ = hash(&msg);
                }

                // Send the hash result back.
                src.send_uni(hash_result.to_vec().into(), 0).await?;

                assert!(!logs_contain("error"));

                num_received += 1;
                // println!("Server received count: {}", num_received);
                if num_received >= num_messages_total {
                    break;
                }
            }

            assert!(!logs_contain("error"));

            Ok(())
        }
    }));

    // Sender + Verifier
    for id in 0..num_senders {
        let messages = sending_msgs.clone();
        assert!(!logs_contain("error"));

        tasks.push(tokio::spawn({
            let (send_endpoint, _, mut recv_incoming_messages, _, _) = new_endpoint().await?;

            async move {
                let mut hash_results = BTreeSet::new();

                let conn = send_endpoint.connect_to(&server_addr).await?;
                for (index, message) in messages.iter().enumerate().take(num_messages_each) {
                    let _ = hash_results.insert(hash(message));

                    info!("sender #{} sending message #{}", id, index);
                    conn.send_uni(message.clone(), 0).await?;
                }

                info!(
                    "sender #{} completed sending messages, starts listening",
                    id
                );

                while let Some((src, msg)) = recv_incoming_messages.next().await {
                    assert!(!logs_contain("error"));

                    info!(
                        "sender #{} received from server {:?} with message size {:?}",
                        id,
                        src.remote_address(),
                        msg
                    );

                    info!("Hash len before: {:?}", hash_results.len());
                    assert!(hash_results.remove(&msg[..]));
                    info!("Hash len after: {:?}", hash_results.len());

                    if hash_results.is_empty() {
                        break;
                    }
                }

                Ok::<_, Report>(())
            }
        }));
    }

    while let Some(result) = tasks.next().await {
        match result {
            Ok(Ok(())) => (),
            other => bail!("Error from test threads: {:?}", other??),
        }
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
#[traced_test]
async fn many_messages() -> Result<()> {
    use futures::future;
    use std::{convert::TryInto, sync::Arc};

    let num_messages: usize = 10_000;

    let (send_endpoint, _, _, _, _) = new_endpoint().await?;
    let (recv_endpoint, _, mut recv_incoming_messages, _, _) = new_endpoint().await?;

    let send_addr = send_endpoint.public_addr();
    let recv_addr = recv_endpoint.public_addr();

    let mut tasks = Vec::new();

    // Sender
    let send_endpoint = Arc::new(send_endpoint);
    let send_connection = Arc::new(send_endpoint.connect_to(&recv_addr).await?);

    for id in 0..num_messages {
        tasks.push(tokio::spawn({
            let send_connection = Arc::clone(&send_connection);
            async move {
                info!("sending {}", id);
                let msg = id.to_le_bytes().to_vec().into();
                send_connection.send_uni(msg, 0).await?;
                info!("sent {}", id);

                Ok::<_, Report>(())
            }
        }));
    }

    // Receiver
    tasks.push(tokio::spawn({
        async move {
            let mut num_received = 0;

            while let Some((src, msg)) = recv_incoming_messages.next().await {
                let id = usize::from_le_bytes(msg[..].try_into().unwrap());
                assert_eq!(src.remote_address(), send_addr);
                info!("received {}", id);

                num_received += 1;

                if num_received >= num_messages {
                    break;
                }
            }

            Ok(())
        }
    }));

    let _ = future::try_join_all(tasks).await?;
    Ok(())
}

// When we bootstrap with multiple bootstrap contacts, we will use the first connection
// that succeeds. We should still be able to establish a connection with the rest of the
// bootstrap contacts later.
#[tokio::test(flavor = "multi_thread")]
async fn connection_attempts_to_bootstrap_contacts_should_succeed() -> Result<()> {
    let (ep1, _, _, _, _) = new_endpoint().await?;
    let (ep2, _, _, _, _) = new_endpoint().await?;
    let (ep3, _, _, _, _) = new_endpoint().await?;

    let contacts = vec![ep1.public_addr(), ep2.public_addr(), ep3.public_addr()];

    let (ep, _, _, _, bootstrapped_peer) = Endpoint::new(
        local_addr(),
        &contacts,
        Config {
            retry_config: RetryConfig {
                retrying_max_elapsed_time: Duration::from_millis(500),
                ..RetryConfig::default()
            },
            ..Config::default()
        },
    )
    .await?;
    let bootstrapped_peer =
        bootstrapped_peer.ok_or_else(|| eyre!("Failed to connecto to any contact"))?;

    for peer in contacts {
        if peer != bootstrapped_peer.remote_address() {
            ep.connect_to(&peer).await.map(drop)?;
        }
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn reachability() -> Result<()> {
    let (ep1, _, _, _, _) = new_endpoint().await?;
    let (ep2, _, _, _, _) = new_endpoint().await?;

    if let Ok(()) = ep1.is_reachable(&"127.0.0.1:12345".parse()?).await {
        eyre!("Unexpected success");
    };
    let reachable_addr = ep2.public_addr();
    ep1.is_reachable(&reachable_addr).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
#[traced_test]
async fn client() -> Result<()> {
    use crate::{Config, Endpoint};

    let (server, _, mut server_messages, _, _) = new_endpoint().await?;
    let (client, mut client_messages, mut client_disconnections) = Endpoint::new_client(
        local_addr(),
        Config {
            idle_timeout: Some(Duration::from_millis(500)),
            retry_config: RetryConfig {
                retrying_max_elapsed_time: Duration::from_millis(500),
                ..RetryConfig::default()
            },
            ..Config::default()
        },
    )?;

    println!("first send");
    client
        .connect_to(&server.public_addr())
        .await?
        .send_uni(b"hello"[..].into(), 0)
        .await?;

    println!("first recv");
    let (sender, message) = server_messages
        .next()
        .await
        .ok_or_else(|| eyre!("Did not receive expected message"))?;
    assert_eq!(sender.remote_address(), client.public_addr());
    assert_eq!(&message[..], b"hello");

    println!("second send");
    sender.send_uni(b"world"[..].into(), 0).await?;
    println!("second recv");
    let (sender, message) = client_messages
        .next()
        .await
        .ok_or_else(|| eyre!("Did not receive expected message"))?;
    assert_eq!(sender.remote_address(), server.public_addr());
    assert_eq!(&message[..], b"world");

    println!("disconnection");
    let disconnector = client_disconnections
        .next()
        .await
        .ok_or_else(|| eyre!("Did not receive expected disconnection"))?;
    assert_eq!(disconnector, server.public_addr());

    Ok(())
}
