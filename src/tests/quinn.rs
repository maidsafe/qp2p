use std::net::SocketAddr;
use std::sync::Arc;

use super::{hash, random_msg};
use crate::{api::bind, config::SerialisableCertificate, peer_config};
use anyhow::Result;
use bytes::Bytes;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use peer_config::{DEFAULT_IDLE_TIMEOUT_MSEC, DEFAULT_KEEP_ALIVE_INTERVAL_MSEC};
use std::collections::BTreeSet;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::sync::RwLock;
use tracing::{error, info, trace, warn};

const DOMAIN: &str = "quinn.test";
const MESSAGE_LEN: usize = 1024;
const CHANNEL_SIZE: usize = 0;

#[derive(Clone)]
enum ChannelSender<T> {
    Unbounded(UnboundedSender<T>),
    Bounded(Sender<T>),
}

impl<T> ChannelSender<T> {
    async fn send(&self, val: T) {
        match self {
            Self::Unbounded(tx) => {
                let _ = tx.send(val);
            }
            Self::Bounded(tx) => {
                let _ = tx.send(val).await;
            }
        }
    }
}

enum ChannelReceiver<T> {
    Unbounded(UnboundedReceiver<T>),
    Bounded(Receiver<T>),
}

impl<T> ChannelReceiver<T> {
    async fn recv(&mut self) -> Option<T> {
        match self {
            ChannelReceiver::Bounded(ref mut rx) => rx.recv().await,
            ChannelReceiver::Unbounded(ref mut rx) => rx.recv().await,
        }
    }
}

#[derive(Clone)]
struct Peer {
    endpoint: quinn::Endpoint,
    client_cfg: quinn::ClientConfig,
    message_tx: ChannelSender<(SocketAddr, Bytes)>,
    connections: Arc<RwLock<Vec<quinn::Connection>>>,
}

impl Peer {
    // Takes the channel size. Set `0` to use an unbounded channel.
    fn new(channel_size: usize) -> Result<(Self, ChannelReceiver<(SocketAddr, Bytes)>)> {
        let (key, cert) = {
            let our_complete_cert = SerialisableCertificate::new(vec![DOMAIN.to_string()])?;
            our_complete_cert.obtain_priv_key_and_cert()?
        };

        let endpoint_cfg = peer_config::new_our_cfg(
            DEFAULT_IDLE_TIMEOUT_MSEC,
            DEFAULT_KEEP_ALIVE_INTERVAL_MSEC,
            cert,
            key,
        )?;

        let client_cfg = peer_config::new_client_cfg(
            DEFAULT_IDLE_TIMEOUT_MSEC,
            DEFAULT_KEEP_ALIVE_INTERVAL_MSEC,
        )?;

        let (endpoint, mut incoming) = bind(endpoint_cfg, "127.0.0.1:0".parse()?, true)?;

        let (message_tx, message_rx) = if channel_size == 0 {
            let (message_tx, message_rx) = unbounded_channel();
            (
                ChannelSender::Unbounded(message_tx),
                ChannelReceiver::Unbounded(message_rx),
            )
        } else {
            let (message_tx, message_rx) = channel(channel_size);
            (
                ChannelSender::Bounded(message_tx),
                ChannelReceiver::Bounded(message_rx),
            )
        };
        let connections = Arc::new(RwLock::new(Vec::new()));
        let conns = connections.clone();
        let message_sender = message_tx.clone();
        let _ = tokio::spawn(async move {
            loop {
                match incoming.next().await {
                    Some(quinn_conn) => match quinn_conn.await {
                        Ok(conn) => {
                            let quinn::NewConnection {
                                connection,
                                uni_streams,
                                ..
                            } = conn;
                            let peer_address = connection.remote_address();
                            let _x = conns.write().await.push(connection);
                            listen_for_messages(uni_streams, peer_address, message_sender.clone());
                        }
                        Err(err) => {
                            error!(
                                "An incoming connection failed because of an error: {:?}",
                                err
                            );
                        }
                    },
                    None => {
                        trace!("quinn::Incoming::next() returned None. There will be no more incoming connections");
                        break;
                    }
                }
            }
        });

        Ok((
            Peer {
                endpoint,
                client_cfg,
                message_tx,
                connections,
            },
            message_rx,
        ))
    }

    fn socket_addr(&self) -> Result<SocketAddr> {
        Ok(self.endpoint.local_addr()?)
    }

    async fn send_message(&mut self, addr: &SocketAddr, msg: Bytes) -> Result<()> {
        let new_conn = self
            .endpoint
            .connect_with(self.client_cfg.clone(), &addr, DOMAIN)?
            .await?;
        let mut stream = new_conn.connection.open_uni().await?;
        let _ = stream.write_all(&msg).await?;
        stream.finish().await?;
        self.connections.write().await.push(new_conn.connection);
        Ok(())
    }

    fn close(&self) {
        self.endpoint.close(0_u8.into(), b"");
    }
}

fn listen_for_messages(
    mut uni_streams: quinn::IncomingUniStreams,
    src: SocketAddr,
    message_tx: ChannelSender<(SocketAddr, Bytes)>,
) {
    let _ = tokio::spawn(async move {
        read_from_stream(&mut uni_streams, src, message_tx).await?;
        trace!("The connection to {:?} has been terminated.", src);
        Ok::<(), anyhow::Error>(())
    });
}

async fn read_from_stream(
    uni_streams: &mut quinn::IncomingUniStreams,
    peer_addr: SocketAddr,
    message_tx: ChannelSender<(SocketAddr, Bytes)>,
) -> Result<()> {
    while let Some(result) = uni_streams.next().await {
        match result {
            Err(quinn::ConnectionError::ApplicationClosed { .. }) => {
                trace!("Connection terminated by peer {:?}.", peer_addr);
                break;
            }
            Err(err) => {
                warn!(
                    "Failed to read incoming message on uni-stream for peer {:?} with error: {:?}",
                    peer_addr, err
                );
                break;
            }
            Ok(recv) => {
                let bytes = recv.read_to_end(1024).await?;
                message_tx.send((peer_addr, Bytes::from(bytes))).await;
            }
        }
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn multiple_connections_with_many_concurrent_messages() -> Result<()> {
    let num_senders: usize = 150;
    let num_messages_each: usize = 100;
    let num_messages_total: usize = num_senders * num_messages_each;

    let (server_endpoint, mut recv_incoming_messages) = Peer::new(CHANNEL_SIZE)?;
    let server_addr = server_endpoint.socket_addr()?;

    let mut tasks = FuturesUnordered::new();

    let server = server_endpoint.clone();
    // UnboundedReceiver
    let server_handle = tokio::spawn(async move {
        info!("Server started");
        let mut num_received = 0;
        while let Some((src, msg)) = recv_incoming_messages.recv().await {
            info!("{} Got message at server. Processing...", num_received);
            assert_eq!(msg.len(), MESSAGE_LEN);

            let mut sending_endpoint = server.clone();

            let _ = tokio::spawn(async move {
                info!("{} Hashing and responding...", num_received);
                // Hash the inputs for couple times to simulate certain workload.
                let hash_result = hash(&msg);
                for _ in 0..5 {
                    let _ = hash(&msg);
                }
                sending_endpoint
                    .send_message(&src, hash_result.to_vec().into())
                    .await
                    .map_err(|err| anyhow::anyhow!("Error here: {:?}", err))?;

                Ok::<_, anyhow::Error>(())
            });

            num_received += 1;
            info!("COUNT: {}", num_received);
            if num_received >= num_messages_total {
                break;
            }
        }

        Ok::<_, anyhow::Error>(())
    });

    let mut senders = Vec::new();

    // UnboundedSender
    for id in 0..num_senders {
        info!("Sender #{}", id);
        let (send_endpoint, mut recv_incoming_messages) = Peer::new(CHANNEL_SIZE)?;
        let hash_results = Arc::new(RwLock::new(BTreeSet::new()));

        let hashes = hash_results.clone();
        let sender = send_endpoint.clone();
        tasks.push(tokio::spawn(async move {
            while let Some((src, msg)) = recv_incoming_messages.recv().await {
                info!(
                    "#{} received from server {:?} with message size {}. {} left!",
                    id,
                    src,
                    msg.len(),
                    hashes.read().await.len()
                );
                assert!(hashes.write().await.remove(&msg[..]));
                if hashes.read().await.is_empty() {
                    break;
                }
            }
            info!("Sender #{} recv complete", id);
            sender.close();
            Ok::<_, anyhow::Error>(())
        }));

        let hashes = hash_results.clone();
        let mut sender = send_endpoint.clone();
        senders.push(send_endpoint.clone());
        tasks.push(tokio::spawn(async move {
            for _ in 0..num_messages_each {
                let message = random_msg(MESSAGE_LEN);
                let _ = hashes.write().await.insert(hash(&message));
                sender
                    .send_message(&server_addr, message.clone())
                    .await
                    .map_err(|err| anyhow::anyhow!("Error now here: {:?}", err))?;
            }
            info!("Sender #{} send complete", id);

            Ok::<_, anyhow::Error>(())
        }));
    }
    tasks.push(server_handle);
    while let Some(result) = tasks.next().await {
        result??
    }
    Ok(())
}
