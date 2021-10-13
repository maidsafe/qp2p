// Copyright 2021 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use super::{hash, local_addr, new_endpoint, random_msg};
use crate::{
    error::{ConnectionError, RecvError},
    Config, Endpoint, RetryConfig,
};
use color_eyre::eyre::{bail, eyre, Report, Result};
use futures::{future, stream::FuturesUnordered, StreamExt};
use std::{collections::BTreeSet, time::Duration};
use tokio::time::timeout;
use tracing::info;
use tracing_test::traced_test;

#[tokio::test(flavor = "multi_thread")]
async fn successful_connection() -> Result<()> {
    let (peer1, mut peer1_incoming_connections, _) = new_endpoint().await?;
    let peer1_addr = peer1.public_addr();

    let (peer2, _, _) = new_endpoint().await?;
    peer2.connect_to(&peer1_addr).await.map(drop)?;
    let peer2_addr = peer2.public_addr();

    if let Some((connection, _)) = peer1_incoming_connections.next().await {
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
    let (peer1, mut peer1_incoming_connections, _) = new_endpoint().await?;
    let peer1_addr = peer1.public_addr();

    let (peer2, _, _) = new_endpoint().await?;
    let peer2_addr = peer2.public_addr();

    // Peer 2 connects and sends a message
    let (connection, _) = peer2.connect_to(&peer1_addr).await?;
    let msg_from_peer2 = random_msg(1024);
    connection.send(msg_from_peer2.clone()).await?;

    // Peer 1 gets an incoming connection
    let mut peer1_incoming_messages =
        if let Some((connection, incoming)) = peer1_incoming_connections.next().await {
            assert_eq!(connection.remote_address(), peer2_addr);
            incoming
        } else {
            bail!("No incoming connection");
        };

    // Peer 2 gets an incoming message
    if let Some(message) = peer1_incoming_messages.next().await? {
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
    let (alice, _, _) = new_endpoint().await?;
    let alice_addr = alice.public_addr();

    let (bob, mut bob_incoming_connections, _) = new_endpoint().await?;
    let bob_addr = bob.public_addr();

    // Connect for the first time and send a message.
    let (a_to_b, _) = alice.connect_to(&bob_addr).await?;
    let msg0 = random_msg(1024);
    a_to_b.send(msg0.clone()).await?;

    // Bob should recieve an incoming connection and message
    let mut bob_incoming_messages =
        if let Some((connection, incoming)) = bob_incoming_connections.next().await {
            assert_eq!(connection.remote_address(), alice_addr);
            incoming
        } else {
            bail!("No incoming connection");
        };

    if let Some(message) = bob_incoming_messages.next().await? {
        assert_eq!(message, msg0);
    } else {
        bail!("No incoming message");
    }

    // Send another message over the same connection
    let msg1 = random_msg(1024);
    a_to_b.send(msg1.clone()).await?;

    // Bob *should not* get an incoming connection since there is already a connection established
    if let Ok(Some((connection, _))) =
        timeout(Duration::from_secs(2), bob_incoming_connections.next()).await
    {
        bail!(
            "Unexpected incoming connection from {}",
            connection.remote_address()
        );
    }

    if let Some(message) = bob_incoming_messages.next().await? {
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
    let (alice, mut alice_incoming_connections, _) = new_endpoint().await?;
    let alice_addr = alice.public_addr();

    let (bob, mut bob_incoming_connections, _) = new_endpoint().await?;
    let bob_addr = bob.public_addr();

    // Connect for the first time and send a message.
    let (a_to_b, mut alice_incoming_messages) = alice.connect_to(&bob_addr).await?;
    let msg0 = random_msg(1024);
    a_to_b.send(msg0.clone()).await?;

    // Bob should recieve an incoming connection and message
    let (b_to_a, mut bob_incoming_messages) = bob_incoming_connections
        .next()
        .await
        .ok_or_else(|| eyre!("No incoming connection"))?;
    assert_eq!(b_to_a.remote_address(), alice_addr);

    if let Some(message) = bob_incoming_messages.next().await? {
        assert_eq!(message, msg0);
    } else {
        bail!("No incoming message");
    }

    // Bob replies over the opened connection
    let msg1 = random_msg(1024);
    b_to_a.send(msg1.clone()).await?;

    // Alice *will not* get an incoming connection since there is already a connection established
    // However, Alice will still get the incoming message
    if let Ok(Some((connection, _))) =
        timeout(Duration::from_secs(2), alice_incoming_connections.next()).await
    {
        bail!(
            "Unexpected incoming connection from {}",
            connection.remote_address()
        );
    }

    if let Some(message) = alice_incoming_messages.next().await? {
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

    let (alice, mut alice_incoming_connections, _) = new_endpoint().await?;
    let alice_addr = alice.public_addr();

    let (bob, mut bob_incoming_connections, _) = new_endpoint().await?;
    let bob_addr = bob.public_addr();

    let ((a_to_b, _), (b_to_a, _)) =
        future::try_join(alice.connect_to(&bob_addr), bob.connect_to(&alice_addr)).await?;

    let mut alice_incoming_messages =
        if let Some((connection, incoming)) = alice_incoming_connections.next().await {
            assert_eq!(connection.remote_address(), bob_addr);
            incoming
        } else {
            bail!("No incoming connection from Bob");
        };

    let mut bob_incoming_messages =
        if let Some((connection, incoming)) = bob_incoming_connections.next().await {
            assert_eq!(connection.remote_address(), alice_addr);
            incoming
        } else {
            bail!("No incoming connectino from Alice");
        };

    let msg0 = random_msg(1024);
    a_to_b.send(msg0.clone()).await?;

    let msg1 = random_msg(1024);
    b_to_a.send(msg1.clone()).await?;

    if let Some(message) = alice_incoming_messages.next().await? {
        assert_eq!(message, msg1);
    } else {
        bail!("Missing incoming message");
    }

    if let Some(message) = bob_incoming_messages.next().await? {
        assert_eq!(message, msg0);
    } else {
        bail!("Missing incoming message");
    }

    // Bob connects to Alice again. This opens a new connection.
    let (b_to_a, _) = bob.connect_to(&alice_addr).await?;

    let mut alice_incoming_messages = if let Ok(Some((connection, incoming))) =
        timeout(Duration::from_secs(60), alice_incoming_connections.next()).await
    {
        assert_eq!(connection.remote_address(), bob_addr);
        incoming
    } else {
        bail!("Missing incoming connection");
    };

    let msg2 = random_msg(1024);
    b_to_a.send(msg2.clone()).await?;

    if let Some(message) = alice_incoming_messages.next().await? {
        assert_eq!(message, msg2);
    }

    assert_eq!(alice.opened_connection_count(), 1);
    assert_eq!(bob.opened_connection_count(), 2);

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn multiple_concurrent_connects_to_the_same_peer() -> Result<()> {
    let (alice, mut alice_incoming_connections, _) = new_endpoint().await?;
    let alice_addr = alice.public_addr();

    let (bob, mut bob_incoming_connections, _) = new_endpoint().await?;
    let bob_addr = bob.public_addr();

    // Try to establish two connections to the same peer at the same time.
    let ((b_to_a, mut bob_incoming_messages), _) =
        future::try_join(bob.connect_to(&alice_addr), bob.connect_to(&alice_addr)).await?;

    // Alice gets two incoming connections (we drop the 2nd)
    let (a_to_b, mut alice_incoming_messages) = alice_incoming_connections
        .next()
        .await
        .ok_or_else(|| eyre!("Missing incoming connection from bob to alice"))?;
    assert_eq!(a_to_b.remote_address(), bob_addr);

    {
        let (a_to_b, _) = alice_incoming_connections
            .next()
            .await
            .ok_or_else(|| eyre!("Missing incoming connection from bob to alice"))?;
        assert_eq!(a_to_b.remote_address(), bob_addr);
    }

    // Send two messages, one from each end
    let msg0 = random_msg(1024);
    a_to_b.send(msg0.clone()).await?;

    let msg1 = random_msg(1024);
    b_to_a.send(msg1.clone()).await?;

    // Bob did not get a new incoming connection
    if let Ok(Some(_)) = timeout(Duration::from_secs(2), bob_incoming_connections.next()).await {
        bail!("Unexpected incoming connection from alice to bob");
    }

    // Both messages are received  at the other end
    if let Some(message) = alice_incoming_messages.next().await? {
        assert_eq!(message, msg1);
    } else {
        bail!("No message received from Bob");
    }

    if let Some(message) = bob_incoming_messages.next().await? {
        assert_eq!(message, msg0);
    } else {
        bail!("No message from alice");
    }

    assert_eq!(alice.opened_connection_count(), 0);
    assert_eq!(bob.opened_connection_count(), 2);

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn multiple_connections_with_many_concurrent_messages() -> Result<()> {
    use futures::future;

    let num_senders: usize = 10;
    let num_messages_each: usize = 100;
    let num_messages_total: usize = 1000;

    let (server_endpoint, mut server_incoming, _) = new_endpoint().await?;
    let server_addr = server_endpoint.public_addr();

    let test_msgs: Vec<_> = (0..num_messages_each).map(|_| random_msg(1024)).collect();
    let sending_msgs = test_msgs.clone();

    let mut tasks = Vec::new();

    // Receiver
    tasks.push(tokio::spawn({
        async move {
            let mut num_received = 0;
            let mut sending_tasks = Vec::new();

            while let Some((connection, mut connection_incoming)) = server_incoming.next().await {
                while let Some(msg) = connection_incoming.next().await? {
                    info!(
                        "received from {:?} with message size {}",
                        connection.remote_address(),
                        msg.len()
                    );
                    assert_eq!(msg.len(), test_msgs[0].len());

                    sending_tasks.push(tokio::spawn({
                        let connection = connection.clone();
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
            }

            let _ = future::try_join_all(sending_tasks).await?;

            Ok(())
        }
    }));

    // Sender
    for id in 0..num_senders {
        let messages = sending_msgs.clone();
        tasks.push(tokio::spawn({
            let (send_endpoint, _, _) = new_endpoint().await?;

            async move {
                let mut hash_results = BTreeSet::new();
                let (connection, mut connection_incoming) =
                    send_endpoint.connect_to(&server_addr).await?;
                for (index, message) in messages.iter().enumerate().take(num_messages_each) {
                    let _ = hash_results.insert(hash(message));
                    info!("sender #{} sending message #{}", id, index);
                    connection.send(message.clone()).await?;
                }

                info!(
                    "sender #{} completed sending messages, starts listening",
                    id
                );

                while let Some(msg) = connection_incoming.next().await? {
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

    let (server_endpoint, mut server_incoming, _) = new_endpoint().await?;
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

            while let Some((connection, mut connection_incoming)) = server_incoming.next().await {
                let mut stop = false;
                while let Some(msg) =
                    connection_incoming
                        .next()
                        .await
                        .or_else(|error| match error {
                            RecvError::ConnectionLost(ConnectionError::Closed(_)) => Ok(None),
                            _ => Err(error),
                        })?
                {
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
                        stop = true;
                    }
                }

                if stop {
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
            let (send_endpoint, _, _) = new_endpoint().await?;

            async move {
                let mut hash_results = BTreeSet::new();

                let (connection, mut connection_incoming) =
                    send_endpoint.connect_to(&server_addr).await?;
                for (index, message) in messages.iter().enumerate().take(num_messages_each) {
                    let _ = hash_results.insert(hash(message));

                    info!("sender #{} sending message #{}", id, index);
                    connection.send(message.clone()).await?;
                }

                info!(
                    "sender #{} completed sending messages, starts listening",
                    id
                );

                while let Some(msg) =
                    connection_incoming
                        .next()
                        .await
                        .or_else(|error| match error {
                            RecvError::ConnectionLost(ConnectionError::Closed(_)) => Ok(None),
                            _ => Err(error),
                        })?
                {
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
#[traced_test]
async fn many_messages() -> Result<()> {
    use futures::future;
    use std::{convert::TryInto, sync::Arc};

    let num_messages: usize = 10_000;

    let (send_endpoint, _, _) = new_endpoint().await?;
    let (recv_endpoint, mut recv_incoming, _) = new_endpoint().await?;

    let send_addr = send_endpoint.public_addr();
    let recv_addr = recv_endpoint.public_addr();

    let mut tasks = Vec::new();

    // Sender
    let send_connection = Arc::new(send_endpoint.connect_to(&recv_addr).await?.0);

    for id in 0..num_messages {
        tasks.push(tokio::spawn({
            let send_connection = send_connection.clone();
            async move {
                info!("sending {}", id);
                let msg = id.to_le_bytes().to_vec().into();
                send_connection.send(msg).await?;
                info!("sent {}", id);

                Ok::<_, Report>(())
            }
        }));
    }

    // drop the local sender handle
    drop(send_connection);

    // Receiver
    tasks.push(tokio::spawn({
        async move {
            let mut num_received = 0;

            if let Some((connection, mut connection_incoming)) = recv_incoming.next().await {
                while let Some(msg) = connection_incoming.next().await? {
                    let id = usize::from_le_bytes(msg[..].try_into().unwrap());
                    assert_eq!(connection.remote_address(), send_addr);

                    info!("received message #{} ({} total)", id, num_received);
                    num_received += 1;

                    if num_received >= num_messages {
                        break;
                    }
                }
            } else {
                bail!("Did not receive expected connection");
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
    let (ep1, _ep1_incoming, _) = new_endpoint().await?;
    let (ep2, _ep2_incoming, _) = new_endpoint().await?;
    let (ep3, _ep3_incoming, _) = new_endpoint().await?;

    let contacts = vec![ep1.public_addr(), ep2.public_addr(), ep3.public_addr()];

    let (ep, _, bootstrapped_peer) = Endpoint::new(
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
    let (bootstrapped_peer, _) =
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
    let (ep1, _, _) = new_endpoint().await?;
    let (ep2, _, _) = new_endpoint().await?;

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

    let (server, mut server_incoming, _) = new_endpoint().await?;
    let client = Endpoint::new_client(
        local_addr(),
        Config {
            retry_config: RetryConfig {
                retrying_max_elapsed_time: Duration::from_millis(500),
                ..RetryConfig::default()
            },
            ..Config::default()
        },
    )?;

    let (server_to_client, mut client_messages) = client.connect_to(&server.public_addr()).await?;
    server_to_client.send(b"hello"[..].into()).await?;

    let (client_to_server, mut server_messages) = server_incoming
        .next()
        .await
        .ok_or_else(|| eyre!("Did not receive expected connection"))?;
    assert_eq!(client_to_server.remote_address(), client.public_addr());

    let message = server_messages
        .next()
        .await?
        .ok_or_else(|| eyre!("Did not receive expected message"))?;
    assert_eq!(&message[..], b"hello");

    client_to_server.send(b"world"[..].into()).await?;
    let message = client_messages
        .next()
        .await?
        .ok_or_else(|| eyre!("Did not receive expected message"))?;
    assert_eq!(server_to_client.remote_address(), server.public_addr());
    assert_eq!(&message[..], b"world");

    assert_eq!(server.opened_connection_count(), 0);
    assert_eq!(client.opened_connection_count(), 1);

    Ok(())
}
