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
use tracing::info;
use tracing_test::traced_test;

#[tokio::test(flavor = "multi_thread")]
async fn successful_connection() -> Result<()> {
    let (peer1, mut peer1_incoming_connections, _, _) = new_endpoint().await?;
    let peer1_addr = peer1.public_addr();

    let (peer2, _, _, _) = new_endpoint().await?;
    peer2.connect_to(&peer1_addr).await.map(drop)?;
    let peer2_addr = peer2.public_addr();

    if let Some(connection) = peer1_incoming_connections.next().await {
        assert_eq!(connection.remote_address(), peer2_addr);
    } else {
        bail!("No incoming connection");
    }

    assert_eq!(peer1.opened_connection_count(), 0);
    assert_eq!(peer2.opened_connection_count(), 1);

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn single_message() -> Result<()> {
    let (peer1, mut peer1_incoming_connections, mut peer1_incoming_messages, _) =
        new_endpoint().await?;
    let peer1_addr = peer1.public_addr();

    let (peer2, _, _, _) = new_endpoint().await?;
    let peer2_addr = peer2.public_addr();

    // Peer 2 connects and sends a message
    let connection = peer2.connect_to(&peer1_addr).await?;
    let msg_from_peer2 = random_msg(1024);
    connection.send(msg_from_peer2.clone()).await?;

    // Peer 1 gets an incoming connection
    if let Some(connection) = peer1_incoming_connections.next().await {
        assert_eq!(connection.remote_address(), peer2_addr);
    } else {
        bail!("No incoming connection");
    }

    // Peer 2 gets an incoming message
    if let Some((connection, message)) = peer1_incoming_messages.next().await {
        assert_eq!(connection.remote_address(), peer2_addr);
        assert_eq!(message, msg_from_peer2);
    } else {
        bail!("No incoming message");
    }

    assert_eq!(peer1.opened_connection_count(), 0);
    assert_eq!(peer2.opened_connection_count(), 1);

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn reuse_outgoing_connection() -> Result<()> {
    let (alice, _, _, _) = new_endpoint().await?;
    let alice_addr = alice.public_addr();

    let (bob, mut bob_incoming_connections, mut bob_incoming_messages, _) = new_endpoint().await?;
    let bob_addr = bob.public_addr();

    // Connect for the first time and send a message.
    let a_to_b = alice.connect_to(&bob_addr).await?;
    let msg0 = random_msg(1024);
    a_to_b.send(msg0.clone()).await?;

    // Bob should recieve an incoming connection and message
    if let Some(connection) = bob_incoming_connections.next().await {
        assert_eq!(connection.remote_address(), alice_addr);
    } else {
        bail!("No incoming connection");
    }

    if let Some((connection, message)) = bob_incoming_messages.next().await {
        assert_eq!(connection.remote_address(), alice_addr);
        assert_eq!(message, msg0);
    } else {
        bail!("No incoming message");
    }

    // Try connecting again and send a message
    let a_to_b = alice.connect_to(&bob_addr).await?;
    let msg1 = random_msg(1024);
    a_to_b.send(msg1.clone()).await?;

    // Bob *should not* get an incoming connection since there is already a connection established
    if let Ok(Some(connection)) = bob_incoming_connections.next().timeout().await {
        bail!(
            "Unexpected incoming connection from {}",
            connection.remote_address()
        );
    }

    if let Some((connection, message)) = bob_incoming_messages.next().await {
        assert_eq!(connection.remote_address(), alice_addr);
        assert_eq!(message, msg1);
    } else {
        bail!("No incoming message");
    }

    assert_eq!(alice.opened_connection_count(), 1);
    assert_eq!(bob.opened_connection_count(), 0);

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn reuse_incoming_connection() -> Result<()> {
    let (alice, mut alice_incoming_connections, mut alice_incoming_messages, _) =
        new_endpoint().await?;
    let alice_addr = alice.public_addr();

    let (bob, mut bob_incoming_connections, mut bob_incoming_messages, _) = new_endpoint().await?;
    let bob_addr = bob.public_addr();

    // Connect for the first time and send a message.
    let a_to_b = alice.connect_to(&bob_addr).await?;
    let msg0 = random_msg(1024);
    a_to_b.send(msg0.clone()).await?;

    // Bob should recieve an incoming connection and message
    if let Some(connection) = bob_incoming_connections.next().await {
        assert_eq!(connection.remote_address(), alice_addr);
    } else {
        bail!("No incoming connection");
    }

    if let Some((connection, message)) = bob_incoming_messages.next().await {
        assert_eq!(connection.remote_address(), alice_addr);
        assert_eq!(message, msg0);
    } else {
        bail!("No incoming message");
    }

    // Bob tries to connect to alice and sends a message
    let b_to_a = bob.connect_to(&alice_addr).await?;
    let msg1 = random_msg(1024);
    b_to_a.send(msg1.clone()).await?;

    // Alice *will not* get an incoming connection since there is already a connection established
    // However, Alice will still get the incoming message
    if let Ok(Some(connection)) = alice_incoming_connections.next().timeout().await {
        bail!(
            "Unexpected incoming connection from {}",
            connection.remote_address()
        );
    }

    if let Some((connection, message)) = alice_incoming_messages.next().await {
        assert_eq!(connection.remote_address(), bob_addr);
        assert_eq!(message, msg1);
    } else {
        bail!("No incoming message");
    }

    assert_eq!(alice.opened_connection_count(), 1);
    assert_eq!(bob.opened_connection_count(), 0);

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn simultaneous_incoming_and_outgoing_connections() -> Result<()> {
    // If both peers call `connect_to` simultaneously (that is, before any of them receives the
    // others connection first), two separate connections are created. This test verifies that
    // everything still works correctly even in this case.

    let (alice, mut alice_incoming_connections, mut alice_incoming_messages, _) =
        new_endpoint().await?;
    let alice_addr = alice.public_addr();

    let (bob, mut bob_incoming_connections, mut bob_incoming_messages, _) = new_endpoint().await?;
    let bob_addr = bob.public_addr();

    let (a_to_b, b_to_a) =
        future::try_join(alice.connect_to(&bob_addr), bob.connect_to(&alice_addr)).await?;

    if let Some(connection) = alice_incoming_connections.next().await {
        assert_eq!(connection.remote_address(), bob_addr);
    } else {
        bail!("No incoming connection from Bob");
    }

    if let Some(connection) = bob_incoming_connections.next().await {
        assert_eq!(connection.remote_address(), alice_addr);
    } else {
        bail!("No incoming connectino from Alice");
    }

    let msg0 = random_msg(1024);
    a_to_b.send(msg0.clone()).await?;

    let msg1 = random_msg(1024);
    b_to_a.send(msg1.clone()).await?;

    if let Some((connection, message)) = alice_incoming_messages.next().await {
        assert_eq!(connection.remote_address(), bob_addr);
        assert_eq!(message, msg1);
    } else {
        bail!("Missing incoming message");
    }

    if let Some((connection, message)) = bob_incoming_messages.next().await {
        assert_eq!(connection.remote_address(), alice_addr);
        assert_eq!(message, msg0);
    } else {
        bail!("Missing incoming message");
    }

    // Drop all connections between Bob and Alice.
    bob.disconnect_from(&alice_addr).await;

    // Bob connects to Alice again. This opens a new connection.
    let b_to_a = bob.connect_to(&alice_addr).await?;

    if let Ok(Some(connection)) = alice_incoming_connections.next().timeout().await {
        assert_eq!(connection.remote_address(), bob_addr);
    } else {
        bail!("Missing incoming connection");
    }

    let msg2 = random_msg(1024);
    b_to_a.send(msg2.clone()).await?;

    if let Some((connection, message)) = alice_incoming_messages.next().await {
        assert_eq!(connection.remote_address(), bob_addr);
        assert_eq!(message, msg2);
    }

    assert_eq!(alice.opened_connection_count(), 1);
    assert_eq!(bob.opened_connection_count(), 2);

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn multiple_concurrent_connects_to_the_same_peer() -> Result<()> {
    let (alice, mut alice_incoming_connections, mut alice_incoming_messages, _) =
        new_endpoint().await?;
    let alice_addr = alice.public_addr();

    let (bob, mut bob_incoming_connections, mut bob_incoming_messages, _) = new_endpoint().await?;
    let bob_addr = bob.public_addr();

    // Try to establish two connections to the same peer at the same time.
    let (b_to_a, _) =
        future::try_join(bob.connect_to(&alice_addr), bob.connect_to(&alice_addr)).await?;

    // Alice get only one incoming connection
    if let Some(connection) = alice_incoming_connections.next().await {
        assert_eq!(connection.remote_address(), bob_addr);
    } else {
        bail!("Missing incoming connection");
    }

    if let Ok(Some(connection)) = alice_incoming_connections.next().timeout().await {
        bail!(
            "Unexpected incoming connection from {}",
            connection.remote_address()
        );
    }

    // Send two messages, one from each end
    let msg0 = random_msg(1024);
    alice
        .connect_to(&bob_addr)
        .await?
        .send(msg0.clone())
        .await?;

    let msg1 = random_msg(1024);
    b_to_a.send(msg1.clone()).await?;

    // Bob did not get a new incoming connection
    if let Ok(Some(_)) = bob_incoming_connections.next().timeout().await {
        bail!("Unexpected incoming connection from alice to bob");
    }

    // Both messages are received  at the other end
    if let Some((connection, message)) = alice_incoming_messages.next().await {
        assert_eq!(connection.remote_address(), bob_addr);
        assert_eq!(message, msg1);
    } else {
        bail!("No message received from Bob");
    }

    if let Some((connection, message)) = bob_incoming_messages.next().await {
        assert_eq!(connection.remote_address(), alice_addr);
        assert_eq!(message, msg0);
    } else {
        bail!("No message from alice");
    }

    assert_eq!(alice.opened_connection_count(), 0);
    assert_eq!(bob.opened_connection_count(), 1);

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn multiple_connections_with_many_concurrent_messages() -> Result<()> {
    use futures::future;

    let num_senders: usize = 10;
    let num_messages_each: usize = 100;
    let num_messages_total: usize = 1000;

    let (server_endpoint, _, mut recv_incoming_messages, _) = new_endpoint().await?;
    let server_addr = server_endpoint.public_addr();

    let test_msgs: Vec<_> = (0..num_messages_each).map(|_| random_msg(1024)).collect();
    let sending_msgs = test_msgs.clone();

    let mut tasks = Vec::new();

    // Receiver
    tasks.push(tokio::spawn({
        async move {
            let mut num_received = 0;
            let mut sending_tasks = Vec::new();

            while let Some((connection, msg)) = recv_incoming_messages.next().await {
                info!(
                    "received from {:?} with message size {}",
                    connection.remote_address(),
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
                        connection.send(hash_result.to_vec().into()).await?;

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
            let (send_endpoint, _, mut recv_incoming_messages, _) = new_endpoint().await?;

            async move {
                let mut hash_results = BTreeSet::new();
                let connection = send_endpoint.connect_to(&server_addr).await?;
                for (index, message) in messages.iter().enumerate().take(num_messages_each) {
                    let _ = hash_results.insert(hash(message));
                    info!("sender #{} sending message #{}", id, index);
                    connection.send(message.clone()).await?;
                }

                info!(
                    "sender #{} completed sending messages, starts listening",
                    id
                );

                while let Some((connection, msg)) = recv_incoming_messages.next().await {
                    info!(
                        "#{} received from server {:?} with message size {}",
                        id,
                        connection.remote_address(),
                        msg.len()
                    );

                    info!("Hash len before: {:?}", hash_results.len());
                    assert!(hash_results.remove(&msg[..]));
                    info!("Hash len after: {:?}", hash_results.len());

                    if hash_results.is_empty() {
                        break;
                    }
                }

                assert_eq!(send_endpoint.opened_connection_count(), 1);

                Ok::<_, Report>(())
            }
        }));
    }

    let _ = future::try_join_all(tasks).await?;

    assert_eq!(server_endpoint.opened_connection_count(), 0);

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
#[traced_test]
async fn multiple_connections_with_many_larger_concurrent_messages() -> Result<()> {
    let num_senders: usize = 10;
    let num_messages_each: usize = 100;
    let num_messages_total: usize = num_senders * num_messages_each;

    let (server_endpoint, _, mut recv_incoming_messages, _) = new_endpoint().await?;
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

            while let Some((connection, msg)) = recv_incoming_messages.next().await {
                info!(
                    "received from {:?} with message size {}",
                    connection.remote_address(),
                    msg.len()
                );
                assert!(!logs_contain("error"));

                assert_eq!(msg.len(), test_msgs[0].len());

                let hash_result = hash(&msg);
                for _ in 0..5 {
                    let _ = hash(&msg);
                }

                // Send the hash result back.
                connection.send(hash_result.to_vec().into()).await?;

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
            let (send_endpoint, _, mut recv_incoming_messages, _) = new_endpoint().await?;

            async move {
                let mut hash_results = BTreeSet::new();

                let connection = send_endpoint.connect_to(&server_addr).await?;
                for (index, message) in messages.iter().enumerate().take(num_messages_each) {
                    let _ = hash_results.insert(hash(message));

                    info!("sender #{} sending message #{}", id, index);
                    connection.send(message.clone()).await?;
                }

                info!(
                    "sender #{} completed sending messages, starts listening",
                    id
                );

                while let Some((connection, msg)) = recv_incoming_messages.next().await {
                    assert!(!logs_contain("error"));

                    info!(
                        "sender #{} received from server {:?} with message size {:?}",
                        id,
                        connection.remote_address(),
                        msg
                    );

                    info!("Hash len before: {:?}", hash_results.len());
                    assert!(hash_results.remove(&msg[..]));
                    info!("Hash len after: {:?}", hash_results.len());

                    if hash_results.is_empty() {
                        break;
                    }
                }

                assert_eq!(send_endpoint.opened_connection_count(), 1);

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

    assert_eq!(server_endpoint.opened_connection_count(), 0);

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn many_messages() -> Result<()> {
    use futures::future;
    use std::{convert::TryInto, sync::Arc};

    let num_messages: usize = 10_000;

    let (send_endpoint, _, _, _) = new_endpoint().await?;
    let (recv_endpoint, _, mut recv_incoming_messages, _) = new_endpoint().await?;

    let send_addr = send_endpoint.public_addr();
    let recv_addr = recv_endpoint.public_addr();

    let mut tasks = Vec::new();

    // Sender
    let send_endpoint = Arc::new(send_endpoint);

    for id in 0..num_messages {
        tasks.push(tokio::spawn({
            let endpoint = send_endpoint.clone();
            async move {
                info!("sending {}", id);
                let msg = id.to_le_bytes().to_vec().into();
                endpoint.connect_to(&recv_addr).await?.send(msg).await?;
                info!("sent {}", id);

                Ok::<_, Report>(())
            }
        }));
    }

    // Receiver
    tasks.push(tokio::spawn({
        async move {
            let mut num_received = 0;

            while let Some((connection, msg)) = recv_incoming_messages.next().await {
                let id = usize::from_le_bytes(msg[..].try_into().unwrap());
                assert_eq!(connection.remote_address(), send_addr);
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

    assert_eq!(send_endpoint.opened_connection_count(), 1);
    assert_eq!(recv_endpoint.opened_connection_count(), 0);

    Ok(())
}

// When we bootstrap with multiple bootstrap contacts, we will use the first connection
// that succeeds. We should still be able to establish a connection with the rest of the
// bootstrap contacts later.
#[tokio::test(flavor = "multi_thread")]
async fn connection_attempts_to_bootstrap_contacts_should_succeed() -> Result<()> {
    let (ep1, _, _, _) = new_endpoint().await?;
    let (ep2, _, _, _) = new_endpoint().await?;
    let (ep3, _, _, _) = new_endpoint().await?;

    let contacts = vec![ep1.public_addr(), ep2.public_addr(), ep3.public_addr()];

    let (ep, _, _, bootstrapped_peer) = Endpoint::<[u8; 32]>::new(
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

    assert_eq!(ep1.opened_connection_count(), 0);
    assert_eq!(ep2.opened_connection_count(), 0);
    assert_eq!(ep3.opened_connection_count(), 0);
    assert_eq!(ep.opened_connection_count(), 3);

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn reachability() -> Result<()> {
    let (ep1, _, _, _) = new_endpoint().await?;
    let (ep2, _, _, _) = new_endpoint().await?;

    if let Ok(()) = ep1.is_reachable(&"127.0.0.1:12345".parse()?).await {
        bail!("Unexpected success");
    };
    let reachable_addr = ep2.public_addr();
    ep1.is_reachable(&reachable_addr).await?;

    assert_eq!(ep1.opened_connection_count(), 0);
    assert_eq!(ep2.opened_connection_count(), 0);

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
#[traced_test]
async fn client() -> Result<()> {
    use crate::{Config, Endpoint};

    let (server, _, mut server_messages, _) = new_endpoint().await?;
    let (client, mut client_messages) = Endpoint::<[u8; 32]>::new_client(
        local_addr(),
        Config {
            retry_config: RetryConfig {
                retrying_max_elapsed_time: Duration::from_millis(500),
                ..RetryConfig::default()
            },
            ..Config::default()
        },
    )?;

    client
        .connect_to(&server.public_addr())
        .await?
        .send(b"hello"[..].into())
        .await?;

    let (client_to_server, message) = server_messages
        .next()
        .await
        .ok_or_else(|| eyre!("Did not receive expected message"))?;
    assert_eq!(client_to_server.remote_address(), client.public_addr());
    assert_eq!(&message[..], b"hello");

    client_to_server.send(b"world"[..].into()).await?;
    let (server_to_client, message) = client_messages
        .next()
        .await
        .ok_or_else(|| eyre!("Did not receive expected message"))?;
    assert_eq!(server_to_client.remote_address(), server.public_addr());
    assert_eq!(&message[..], b"world");

    server.disconnect_from(&client.public_addr()).await;

    // sending should now fail, since the connection was closed at the peer
    match client_to_server.send(b"world"[..].into()).await {
        Err(crate::SendError::ConnectionLost(_)) => {}
        result => bail!(
            "expected connection loss when sending message, but got: {:?}",
            result
        ),
    }

    // reconnecting should increment client's opened connection count
    let _ = client.connect_to(&server.public_addr()).await?;

    assert_eq!(server.opened_connection_count(), 0);
    assert_eq!(client.opened_connection_count(), 2);

    Ok(())
}

trait Timeout: Sized {
    fn timeout(self) -> tokio::time::Timeout<Self>;
}

impl<F: std::future::Future> Timeout for F {
    fn timeout(self) -> tokio::time::Timeout<Self> {
        tokio::time::timeout(Duration::from_secs(2), self)
    }
}
