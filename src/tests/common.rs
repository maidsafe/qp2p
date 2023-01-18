// Copyright 2021 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use super::{hash, local_addr, new_endpoint, new_endpoint_with_keepalive, random_msg};
use crate::SendError;
use bytes::Bytes;
use color_eyre::eyre::{bail, eyre, Report, Result};
use futures::future;
use std::sync::Arc;
use std::{collections::BTreeSet, time::Duration};
use tracing::info;
// use tracing_test::traced_test;

#[tokio::test(flavor = "multi_thread")]
async fn successful_connection() -> Result<()> {
    let (peer1, mut peer1_incoming_connections) = new_endpoint_with_keepalive().await?;
    let peer1_addr = peer1.local_addr();

    let (peer2, _) = new_endpoint_with_keepalive().await?;
    peer2.connect_to(&peer1_addr).await.map(drop)?;
    let peer2_addr = peer2.local_addr();

    if let Ok(Some((connection, _))) = peer1_incoming_connections.next().timeout().await {
        assert_eq!(connection.remote_address(), peer2_addr);
    } else {
        bail!("No incoming connection");
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn single_message() -> Result<()> {
    let (peer1, mut peer1_incoming_connections) = new_endpoint_with_keepalive().await?;
    let peer1_addr = peer1.local_addr();

    let (peer2, _) = new_endpoint_with_keepalive().await?;
    let peer2_addr = peer2.local_addr();

    // Peer 2 connects and sends a message
    let (connection, _) = peer2.connect_to(&peer1_addr).await?;
    let msg_from_peer2 = random_msg(1024);
    connection
        .send((Bytes::new(), Bytes::new(), msg_from_peer2.clone()))
        .await?;

    // Peer 1 gets an incoming connection
    let (mut peer1_incoming_messages, _conn) = if let Ok(Some((connection, incoming))) =
        peer1_incoming_connections.next().timeout().await
    {
        assert_eq!(connection.remote_address(), peer2_addr);
        (incoming, connection)
    } else {
        bail!("No incoming connection");
    };

    // Peer 2 gets an incoming message
    if let Ok(message) = peer1_incoming_messages.next().timeout().await {
        assert_eq!(message?, Some((Bytes::new(), Bytes::new(), msg_from_peer2)));
    } else {
        bail!("No incoming message");
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn no_reuse_outgoing_connection() -> Result<()> {
    let (alice, _) = new_endpoint_with_keepalive().await?;
    let alice_addr = alice.local_addr();

    let (bob, mut bob_incoming_connections) = new_endpoint_with_keepalive().await?;
    let bob_addr = bob.local_addr();

    // Connect for the first time and send a message.
    let (a_to_b, _) = alice.connect_to(&bob_addr).await?;
    let msg0 = random_msg(1024);
    a_to_b
        .send((Bytes::new(), Bytes::new(), msg0.clone()))
        .await?;

    // Bob should recieve an incoming connection and message
    let (_conn, mut bob_incoming_messages) =
        if let Ok(Some((connection, incoming))) = bob_incoming_connections.next().timeout().await {
            assert_eq!(connection.remote_address(), alice_addr);
            (connection, incoming)
        } else {
            bail!("No incoming connection");
        };

    if let Ok(message) = bob_incoming_messages.next().timeout().await {
        assert_eq!(message?, Some((Bytes::new(), Bytes::new(), msg0)));
    } else {
        bail!("No incoming message");
    }

    // Try connecting again and send a message
    let (a_to_b, _) = alice.connect_to(&bob_addr).await?;
    let msg1 = random_msg(1024);
    a_to_b
        .send((Bytes::new(), Bytes::new(), msg1.clone()))
        .await?;

    // bob gets nothing on the original connection
    if bob_incoming_messages.next().timeout().await.is_ok() {
        bail!("Received unexpected message");
    }

    // Bob gets a new incoming connection
    let mut bob_incoming_messages =
        if let Ok(Some((connection, incoming))) = bob_incoming_connections.next().timeout().await {
            assert_eq!(connection.remote_address(), alice_addr);
            incoming
        } else {
            bail!("No incoming connection");
        };

    if let Ok(message) = bob_incoming_messages.next().timeout().await {
        assert_eq!(message?, Some((Bytes::new(), Bytes::new(), msg1)));
    } else {
        bail!("No incoming message");
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn no_reuse_incoming_connection() -> Result<()> {
    let (alice, mut alice_incoming_connections) = new_endpoint_with_keepalive().await?;
    let alice_addr = alice.local_addr();

    let (bob, mut bob_incoming_connections) = new_endpoint_with_keepalive().await?;
    let bob_addr = bob.local_addr();

    // Connect for the first time and send a message.
    let (a_to_b, mut alice_incoming_messages) = alice.connect_to(&bob_addr).await?;
    let msg0 = random_msg(1024);
    a_to_b
        .send((Bytes::new(), Bytes::new(), msg0.clone()))
        .await?;

    // Bob should recieve an incoming connection and message
    let (_conn, mut bob_incoming_messages) =
        if let Ok(Some((connection, incoming))) = bob_incoming_connections.next().timeout().await {
            assert_eq!(connection.remote_address(), alice_addr);
            (connection, incoming)
        } else {
            bail!("No incoming connection");
        };

    if let Ok(message) = bob_incoming_messages.next().timeout().await {
        assert_eq!(message?, Some((Bytes::new(), Bytes::new(), msg0)));
    } else {
        bail!("No incoming message");
    }

    // Bob tries to connect to alice and sends a message
    let (b_to_a, _) = bob.connect_to(&alice_addr).await?;
    let msg1 = random_msg(1024);
    b_to_a
        .send((Bytes::new(), Bytes::new(), msg1.clone()))
        .await?;

    // Alice receives no message over the original connection
    if alice_incoming_messages.next().timeout().await.is_ok() {
        bail!("Received unexpected message");
    }

    // Alice gets a new incoming connection, and receives the incoming message over it
    let mut alice_incoming_messages = if let Ok(Some((connection, incoming))) =
        alice_incoming_connections.next().timeout().await
    {
        assert_eq!(connection.remote_address(), bob_addr);
        incoming
    } else {
        bail!("No incoming connection from bob to alice");
    };

    if let Ok(message) = alice_incoming_messages.next().timeout().await {
        assert_eq!(message?, Some((Bytes::new(), Bytes::new(), msg1)));
    } else {
        bail!("No incoming message");
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn simultaneous_incoming_and_outgoing_connections() -> Result<()> {
    // If both peers call `connect_to` simultaneously (that is, before any of them receives the
    // others connection first), two separate connections are created. This test verifies that
    // everything still works correctly even in this case.

    let (alice, mut alice_incoming_connections) = new_endpoint_with_keepalive().await?;
    let alice_addr = alice.local_addr();

    let (bob, mut bob_incoming_connections) = new_endpoint_with_keepalive().await?;
    let bob_addr = bob.local_addr();

    let ((a_to_b, _), (b_to_a, _)) =
        future::try_join(alice.connect_to(&bob_addr), bob.connect_to(&alice_addr)).await?;

    let (_conn_to_alice, mut alice_incoming_messages) = if let Ok(Some((connection, incoming))) =
        alice_incoming_connections.next().timeout().await
    {
        assert_eq!(connection.remote_address(), bob_addr);
        (connection, incoming)
    } else {
        bail!("No incoming connection from Bob");
    };

    let (_conn_to_bob, mut bob_incoming_messages) =
        if let Ok(Some((connection, incoming))) = bob_incoming_connections.next().timeout().await {
            assert_eq!(connection.remote_address(), alice_addr);
            (connection, incoming)
        } else {
            bail!("No incoming connection from Alice");
        };

    let msg0 = random_msg(1024);
    a_to_b
        .send((Bytes::new(), Bytes::new(), msg0.clone()))
        .await?;

    let msg1 = random_msg(1024);
    b_to_a
        .send((Bytes::new(), Bytes::new(), msg1.clone()))
        .await?;

    if let Ok(message) = alice_incoming_messages.next().timeout().await {
        assert_eq!(message?, Some((Bytes::new(), Bytes::new(), msg1)));
    } else {
        bail!("Missing incoming message");
    }

    if let Ok(message) = bob_incoming_messages.next().timeout().await {
        assert_eq!(message?, Some((Bytes::new(), Bytes::new(), msg0)));
    } else {
        bail!("Missing incoming message");
    }

    // Drop all connections between Bob and Alice.
    drop(b_to_a);
    drop(bob_incoming_messages);

    // Bob connects to Alice again. This opens a new connection.
    let (b_to_a, _) = bob.connect_to(&alice_addr).await?;

    let (_conn, mut alice_incoming_messages) = if let Ok(Some((connection, incoming))) =
        alice_incoming_connections.next().timeout().await
    {
        assert_eq!(connection.remote_address(), bob_addr);
        (connection, incoming)
    } else {
        bail!("Missing incoming connection");
    };

    let msg2 = random_msg(1024);
    b_to_a
        .send((Bytes::new(), Bytes::new(), msg2.clone()))
        .await?;

    if let Ok(message) = alice_incoming_messages.next().timeout().await {
        assert_eq!(message?, Some((Bytes::new(), Bytes::new(), msg2)));
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn multiple_concurrent_connects_to_the_same_peer() -> Result<()> {
    let (alice, mut alice_incoming_connections) = new_endpoint_with_keepalive().await?;
    let alice_addr = alice.local_addr();

    let (bob, mut bob_incoming_connections) = new_endpoint_with_keepalive().await?;
    let bob_addr = bob.local_addr();

    let (carol, mut carol_incoming_connections) = new_endpoint_with_keepalive().await?;
    let carol_addr = carol.local_addr();

    let msg0 = random_msg(1024);
    let msg1 = random_msg(1024);
    let msg2 = random_msg(1024);

    let (b_to_a, mut b_incoming_from_a) = bob.connect_to(&alice_addr).await?;
    let (c_to_a, mut c_incoming_from_a) = carol.connect_to(&alice_addr).await?;

    if let Some((a_to_b, mut alice_incoming_messages)) = alice_incoming_connections.next().await {
        if let Some((a_to_c, mut alice_incoming_messages_2)) =
            alice_incoming_connections.next().await
        {
            // all cehcks need to happen within here as conns are kept alive inside this if block

            assert_eq!(a_to_b.remote_address(), bob_addr);
            assert_eq!(a_to_c.remote_address(), carol_addr);

            b_to_a
                .send((Bytes::new(), Bytes::new(), msg0.clone()))
                .await?;
            c_to_a
                .send((Bytes::new(), Bytes::new(), msg1.clone()))
                .await?;

            // check bob hasnt been connected to for some reason
            if let Ok(Some(_)) = bob_incoming_connections.next().timeout().await {
                bail!("Unexpected incoming connection from alice to bob");
            }
            // check carol hasnt been connected to for some reason
            if let Ok(Some(_)) = carol_incoming_connections.next().timeout().await {
                bail!("Unexpected incoming connection from alice to carol");
            }

            // Both messages are received  at the alice end
            if let Ok(message) = alice_incoming_messages.next().timeout().await {
                assert_eq!(message?, Some((Bytes::new(), Bytes::new(), msg0)));
            } else {
                bail!("No message received from Bob");
            }

            if let Ok(message) = alice_incoming_messages_2.next().timeout().await {
                assert_eq!(message?, Some((Bytes::new(), Bytes::new(), msg1)));
            } else {
                bail!("No message from carol");
            }

            a_to_b
                .send((Bytes::new(), Bytes::new(), msg2.clone()))
                .await?;
            a_to_c
                .send((Bytes::new(), Bytes::new(), msg2.clone()))
                .await?;

            if let Ok(message) = b_incoming_from_a.next().timeout().await {
                assert_eq!(message?, Some((Bytes::new(), Bytes::new(), msg2.clone())));
            } else {
                bail!("No message received from Alice to bob");
            }
            if let Ok(message) = c_incoming_from_a.next().timeout().await {
                assert_eq!(message?, Some((Bytes::new(), Bytes::new(), msg2)));
            } else {
                bail!("No message received from Alive to Carol");
            }
        }
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn multiple_connections_with_many_concurrent_messages() -> Result<()> {
    use futures::future;

    let num_senders: usize = 100;
    let num_messages_each: usize = 100;
    let num_messages_total: usize = num_senders * num_messages_each;

    let (server_endpoint, mut recv_incoming_connections) = new_endpoint_with_keepalive().await?;
    let server_addr = server_endpoint.local_addr();
    let msg_len = 200;
    let test_msgs: Vec<_> = (0..num_messages_each)
        .map(|_| random_msg(msg_len))
        .collect();
    assert_eq!(test_msgs.len(), num_messages_each);
    assert_eq!(msg_len, test_msgs[0].len());

    let sending_msgs = Arc::new(test_msgs);

    let mut tasks = Vec::new();

    // Receiver
    tasks.push(tokio::spawn({
        async move {
            let mut num_received = 0;
            let mut sending_tasks = Vec::new();
            let mut task_connection = None;
            while let Some((connection, mut recv_incoming_messages)) =
                recv_incoming_connections.next().await
            {
                let connection = Arc::new(connection);
                while let Ok(Ok(Some((_, _, msg)))) = recv_incoming_messages.next().timeout().await
                {
                    info!(
                        "received from {:?} with message size {}",
                        connection.remote_address(),
                        msg.len()
                    );
                    task_connection = Some(connection.clone());
                    sending_tasks.push(tokio::spawn({
                        let connection = connection.clone();
                        async move {
                            let hash_result = hash(&msg);
                            // to simulate certain workload.
                            for _ in 0..5 {
                                let _ = hash(&msg);
                            }

                            // Send the hash result back.
                            info!("About to send hash from receiver");
                            connection
                                .send((Bytes::new(), Bytes::new(), hash_result.to_vec().into()))
                                .await?;

                            Ok::<_, Report>(())
                        }
                    }));

                    num_received += 1;
                }
                if num_received >= num_messages_total {
                    break;
                }
            }

            let _res = future::try_join_all(sending_tasks).await?;

            info!("Receiver closed");

            Ok(task_connection)
        }
    }));

    // Sender
    for id in 0..num_senders {
        let messages = sending_msgs.clone();
        tasks.push(tokio::spawn({
            let (send_endpoint, _) = new_endpoint_with_keepalive().await?;
            let task_connection = None;

            async move {
                let mut hash_results = BTreeSet::new();
                let (connection, mut recv_incoming_messages) =
                    send_endpoint.connect_to(&server_addr).await?;

                assert_eq!(messages.len(), num_messages_each);
                for (index, message) in messages.iter().enumerate() {
                    let _ = hash_results.insert(hash(message));
                    info!("sender #{} sending message #{}", id, index);
                    connection
                        .send((Bytes::new(), Bytes::new(), message.clone()))
                        .await?;
                }

                info!(
                    "sender #{} completed sending messages, starts listening",
                    id
                );
                let mut all_received_msgs = 0;

                while let Ok(Ok(Some((_, _, msg)))) = recv_incoming_messages.next().timeout().await
                {
                    info!(
                        "#{} received from server {:?} with message size {}",
                        id,
                        connection.remote_address(),
                        msg.len()
                    );

                    info!("Hash len before: {:?}", hash_results.len());
                    assert!(hash_results.remove(&msg[..]));
                    info!("Hash len after: {:?}", hash_results.len());
                    all_received_msgs += 1;
                    if hash_results.is_empty() && all_received_msgs == num_messages_each {
                        break;
                    }
                }

                Ok::<_, Report>(task_connection)
            }
        }));
    }

    let _res = future::try_join_all(tasks).await?;

    Ok(())
}

#[tracing_test::traced_test]
#[tokio::test(flavor = "multi_thread")]
async fn multiple_connections_with_many_larger_concurrent_messages() -> Result<()> {
    use futures::future;

    let num_senders: usize = 10;
    let num_messages_each: usize = 100;
    let num_messages_total: usize = num_senders * num_messages_each;

    let (server_endpoint, mut recv_incoming_connections) = new_endpoint_with_keepalive().await?;
    let server_addr = server_endpoint.local_addr();
    let msg_len = 1024 * 1024;
    let test_msgs: Vec<_> = (0..num_messages_each)
        .map(|_| random_msg(msg_len))
        .collect();
    assert_eq!(test_msgs.len(), num_messages_each);
    assert_eq!(msg_len, test_msgs[0].len());

    let sending_msgs = Arc::new(test_msgs);

    let mut tasks = Vec::new();

    // Receiver
    tasks.push(tokio::spawn({
        async move {
            let mut num_received = 0;
            let mut sending_tasks = Vec::new();
            let mut task_connection = None;
            while let Some((connection, mut recv_incoming_messages)) =
                recv_incoming_connections.next().await
            {
                let connection = Arc::new(connection);
                while let Ok(Ok(Some((_, _, msg)))) = recv_incoming_messages.next().timeout().await
                {
                    info!(
                        "received from {:?} with message size {}",
                        connection.remote_address(),
                        msg.len()
                    );
                    task_connection = Some(connection.clone());
                    sending_tasks.push(tokio::spawn({
                        let connection = connection.clone();
                        async move {
                            let hash_result = hash(&msg);
                            // to simulate certain workload.
                            for _ in 0..5 {
                                let _ = hash(&msg);
                            }

                            // Send the hash result back.
                            info!("About to send hash from receiver");
                            connection
                                .send((Bytes::new(), Bytes::new(), hash_result.to_vec().into()))
                                .await?;

                            Ok::<_, Report>(())
                        }
                    }));

                    num_received += 1;
                }
                if num_received >= num_messages_total {
                    break;
                }
            }

            let _res = future::try_join_all(sending_tasks).await?;

            info!("RECEIVER CLOSED");

            Ok(task_connection)
        }
    }));

    // Sender
    for id in 0..num_senders {
        let messages = sending_msgs.clone();
        tasks.push(tokio::spawn({
            let (send_endpoint, _) = new_endpoint_with_keepalive().await?;
            let task_connection = None;

            async move {
                let mut hash_results = BTreeSet::new();
                let (connection, mut recv_incoming_messages) =
                    send_endpoint.connect_to(&server_addr).await?;

                assert_eq!(messages.len(), num_messages_each);
                for (index, message) in messages.iter().enumerate() {
                    let _ = hash_results.insert(hash(message));
                    info!("sender #{} sending message #{}", id, index);
                    connection
                        .send((Bytes::new(), Bytes::new(), message.clone()))
                        .await?;
                }

                info!(
                    "sender #{} completed sending messages, starts listening",
                    id
                );

                let mut all_received_msgs = 0;
                while let Ok(Ok(Some((_, _, msg)))) = recv_incoming_messages.next().timeout().await
                {
                    info!(
                        "#{} received from server {:?} with message size {}",
                        id,
                        connection.remote_address(),
                        msg.len()
                    );

                    info!("Hash len before: {:?}", hash_results.len());
                    assert!(hash_results.remove(&msg[..]));
                    info!("Hash len after: {:?}", hash_results.len());

                    all_received_msgs += 1;
                    if hash_results.is_empty() && all_received_msgs == num_messages_each {
                        break;
                    }
                }

                Ok::<_, Report>(task_connection)
            }
        }));
    }

    let _res = future::try_join_all(tasks).await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn many_messages() -> Result<()> {
    use futures::future;
    use std::{convert::TryInto, sync::Arc};

    let num_messages: usize = 10_000;

    let (send_endpoint, _send_incoming) = new_endpoint_with_keepalive().await?;
    let (recv_endpoint, mut recv_incoming_connections) = new_endpoint_with_keepalive().await?;

    let send_addr = send_endpoint.local_addr();
    let recv_addr = recv_endpoint.local_addr();

    let mut tasks = Vec::new();

    // Sender
    let (sender, _) = send_endpoint.connect_to(&recv_addr).await?;
    let sender = Arc::new(sender);

    for id in 0..num_messages {
        tasks.push(tokio::spawn({
            let sender = sender.clone();
            async move {
                info!("sending {}", id);
                let msg = id.to_le_bytes().to_vec().into();
                sender.send((Bytes::new(), Bytes::new(), msg)).await?;
                info!("sent {}", id);

                Ok::<_, Report>(())
            }
        }));
    }

    // Receiver
    tasks.push(tokio::spawn({
        async move {
            let mut num_received = 0;

            while let Some((connection, mut recv_incoming_messages)) =
                recv_incoming_connections.next().await
            {
                assert_eq!(connection.remote_address(), send_addr);

                while let Ok(Ok(Some((_, _, msg)))) = recv_incoming_messages.next().timeout().await
                {
                    let id = usize::from_le_bytes(msg[..].try_into().unwrap());
                    info!("received {}", id);

                    num_received += 1;
                }

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

#[tokio::test(flavor = "multi_thread")]
async fn reachability() -> Result<()> {
    let (ep1, _ep1_incoming) = new_endpoint_with_keepalive().await?;
    let (ep2, _ep2_incoming) = new_endpoint_with_keepalive().await?;

    if let Ok(()) = ep1.is_reachable(&"127.0.0.1:12345".parse()?).await {
        bail!("Unexpected success");
    };
    let reachable_addr = ep2.local_addr();
    ep1.is_reachable(&reachable_addr).await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn client() -> Result<()> {
    use crate::{Config, Endpoint};

    let (server, mut server_connections) = new_endpoint_with_keepalive().await?;
    let client = Endpoint::new_client(local_addr(), Config::default())?;

    let (client_to_server, mut client_messages) = client.connect_to(&server.local_addr()).await?;
    client_to_server
        .send((Bytes::new(), Bytes::new(), b"hello"[..].into()))
        .await?;

    let (server_to_client, mut server_messages) = server_connections
        .next()
        .await
        .ok_or_else(|| eyre!("did not receive expected connection"))?;

    assert_eq!(server_to_client.remote_address(), client.local_addr());

    let (_, _, message) = server_messages
        .next()
        .timeout()
        .await
        .ok()
        .and_then(Result::transpose)
        .ok_or_else(|| eyre!("Did not receive expected message"))??;
    assert_eq!(&message[..], b"hello");

    server_to_client
        .send((Bytes::new(), Bytes::new(), b"world"[..].into()))
        .await?;

    let (_h, _d, message) = client_messages
        .next()
        .timeout()
        .await
        .ok()
        .and_then(Result::transpose)
        .ok_or_else(|| eyre!("Did not receive expected message"))??;
    assert_eq!(&message[..], b"world");

    drop(server_to_client);
    drop(server_messages);

    // small way for all drop effects to propagate
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // sending should now fail, since the connection was closed at the peer
    match client_to_server
        .send((Bytes::new(), Bytes::new(), b"world"[..].into()))
        .await
    {
        Err(crate::SendError::ConnectionLost(_)) => {}
        result => bail!(
            "expected connection loss when sending message, but got: {:?}",
            result
        ),
    }

    // reconnecting should increment client's opened connection count
    let _ = client.connect_to(&server.local_addr()).await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn no_client_keep_alive_times_out() -> Result<()> {
    use crate::{Config, Endpoint};

    let (server, mut server_connections) = new_endpoint().await?;
    let client = Endpoint::new_client(local_addr(), Config::default())?;

    let (client_to_server, _client_messages) = client.connect_to(&server.local_addr()).await?;
    client_to_server
        .send((Bytes::new(), Bytes::new(), b"hello"[..].into()))
        .await?;

    let (server_to_client, mut server_messages) = server_connections
        .next()
        .await
        .ok_or_else(|| eyre!("did not receive expected connection"))?;

    assert_eq!(server_to_client.remote_address(), client.local_addr());

    let (_, _, message) = server_messages
        .next()
        .timeout()
        .await
        .ok()
        .and_then(Result::transpose)
        .ok_or_else(|| eyre!("Did not receive expected message"))??;
    assert_eq!(&message[..], b"hello");

    // Idle timeout + 2 second wait
    tokio::time::sleep(crate::config::DEFAULT_IDLE_TIMEOUT.saturating_add(Duration::from_secs(2)))
        .await;

    let err = server_to_client
        .send((Bytes::new(), Bytes::new(), b"world"[..].into()))
        .await;

    match err {
        Err(SendError::ConnectionLost(crate::ConnectionError::TimedOut)) => Ok(()),
        _ => bail!("Unexpected outcome trying to send on a conneciton that should have timed out"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn client_keep_alive_works() -> Result<()> {
    use crate::{Config, Endpoint};

    let (server, mut server_connections) = new_endpoint().await?;
    let client = Endpoint::new_client(
        local_addr(),
        Config {
            keep_alive_interval: Some(Duration::from_secs(5)),
            ..Config::default()
        },
    )?;

    let (client_to_server, mut client_messages) = client.connect_to(&server.local_addr()).await?;
    client_to_server
        .send((Bytes::new(), Bytes::new(), b"hello"[..].into()))
        .await?;

    let (server_to_client, mut server_messages) = server_connections
        .next()
        .await
        .ok_or_else(|| eyre!("did not receive expected connection"))?;

    assert_eq!(server_to_client.remote_address(), client.local_addr());

    let (_, _, message) = server_messages
        .next()
        .timeout()
        .await
        .ok()
        .and_then(Result::transpose)
        .ok_or_else(|| eyre!("Did not receive expected message"))??;
    assert_eq!(&message[..], b"hello");

    // Idle timeout and a 2 second wait
    tokio::time::sleep(crate::config::DEFAULT_IDLE_TIMEOUT.saturating_add(Duration::from_secs(2)))
        .await;

    server_to_client
        .send((Bytes::new(), Bytes::new(), b"world"[..].into()))
        .await
        .or_else(|e| bail!("Could not send expected message after wait: {e:?}"))?;

    let (_h, _d, message) = client_messages
        .next()
        .timeout()
        .await
        .ok()
        .and_then(Result::transpose)
        .ok_or_else(|| eyre!("Did not receive expected message after wait"))??;
    assert_eq!(&message[..], b"world");

    Ok(())
}

trait Timeout: Sized {
    fn timeout(self) -> tokio::time::Timeout<Self>;
}

impl<F: std::future::Future> Timeout for F {
    fn timeout(self) -> tokio::time::Timeout<Self> {
        tokio::time::timeout(Duration::from_secs(10), self)
    }
}
