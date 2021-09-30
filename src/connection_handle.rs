// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use super::{
    connection_pool::{ConnId, ConnectionPool, ConnectionRemover},
    error::{ConnectionError, SendError, StreamError},
};
use crate::{
    connection::{Connection, ConnectionIncoming, RecvStream, SendStream},
    Endpoint, RetryConfig,
};
use bytes::Bytes;
use futures::stream::StreamExt;
use std::{fmt::Debug, net::SocketAddr, sync::Arc};
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::{trace, warn};

/// A connection between two [`Endpoint`]s.
///
/// This is backed by an `Arc` and a small amount of metadata, so cloning is fairly cheap. The
/// connection is also pooled, meaning the same underlying connection will be used when connecting
/// multiple times to the same peer. If an error occurs on the connection, it will be removed from
/// the pool. See the documentation of [`Endpoint::connect_to`] for more details about connection
/// pooling.
///
/// [`Endpoint`]: crate::Endpoint
/// [`Endpoint::connect_to`]: crate::Endpoint::connect_to
#[derive(Clone)]
pub struct ConnectionHandle<I: ConnId> {
    inner: Connection,
    default_retry_config: Arc<RetryConfig>,
    remover: ConnectionRemover<I>,
}

/// Disconnection events, and the result that led to disconnection.
#[derive(Debug)]
pub struct DisconnectionEvents(pub Receiver<SocketAddr>);

/// Disconnection
impl DisconnectionEvents {
    /// Blocks until there is a disconnection event and returns the address of the disconnected peer
    pub async fn next(&mut self) -> Option<SocketAddr> {
        self.0.recv().await
    }
}

impl<I: ConnId> ConnectionHandle<I> {
    pub(crate) fn new(
        inner: Connection,
        default_retry_config: Arc<RetryConfig>,
        remover: ConnectionRemover<I>,
    ) -> Self {
        Self {
            inner,
            default_retry_config,
            remover,
        }
    }

    /// Get the ID under which the connection is stored in the pool.
    pub fn id(&self) -> I {
        self.remover.id()
    }

    /// Get the address of the connected peer.
    pub fn remote_address(&self) -> SocketAddr {
        self.inner.remote_address()
    }

    /// Send a message to the peer with default configuration.
    ///
    /// The message will be sent on a unidirectional QUIC stream, meaning the application is
    /// responsible for correlating any anticipated responses from incoming streams.
    ///
    /// The priority will be `0` and retry behaviour will be determined by the
    /// [`Config`](crate::Config) that was used to construct the [`Endpoint`] this connection
    /// belongs to. See [`send_message_with`](Self::send_message_with) if you want to send a message
    /// with specific configuration.
    pub async fn send(&self, msg: Bytes) -> Result<(), SendError> {
        self.send_with(msg, 0, None).await
    }

    /// Send a message to the peer using the given configuration.
    ///
    /// See [`send_message`](Self::send_message) if you want to send with the default configuration.
    pub async fn send_with(
        &self,
        msg: Bytes,
        priority: i32,
        retry_config: Option<&RetryConfig>,
    ) -> Result<(), SendError> {
        retry_config
            .unwrap_or_else(|| self.default_retry_config.as_ref())
            .retry(|| async { Ok(self.send_uni(msg.clone(), priority).await?) })
            .await?;

        Ok(())
    }

    /// Priority default is 0. Both lower and higher can be passed in.
    pub(crate) async fn open_bi(
        &self,
        priority: i32,
    ) -> Result<(SendStream, RecvStream), ConnectionError> {
        let (send_stream, recv_stream) = self.handle_error(self.inner.open_bi().await).await?;
        send_stream.set_priority(priority);
        Ok((send_stream, recv_stream))
    }

    /// Send message to peer using a uni-directional stream.
    /// Priority default is 0. Both lower and higher can be passed in.
    pub(crate) async fn send_uni(&self, msg: Bytes, priority: i32) -> Result<(), SendError> {
        let mut send_stream = self.handle_error(self.inner.open_uni().await).await?;

        // quinn returns `UnknownStream` error if the stream does not exist. We ignore it, on the
        // basis that operations on the stream will fail instead (and the effect of setting priority
        // or not is only observable if the stream exists).
        let _ = send_stream.set_priority(priority);

        self.handle_error(send_stream.send_user_msg(msg).await)
            .await?;

        // We try to make sure the stream is gracefully closed and the bytes get sent,
        // but if it was already closed (perhaps by the peer) then we
        // don't remove the connection from the pool.
        match send_stream.finish().await {
            Ok(()) | Err(SendError::StreamLost(StreamError::Stopped(_))) => Ok(()),
            Err(err) => {
                self.handle_error(Err(err)).await?;
                Ok(())
            }
        }
    }

    async fn handle_error<T, E>(&self, result: Result<T, E>) -> Result<T, E> {
        if result.is_err() {
            self.remover.remove().await
        }

        result
    }
}

pub(super) fn listen_for_incoming_connections<I: ConnId>(
    mut quinn_incoming: quinn::Incoming,
    connection_pool: ConnectionPool<I>,
    message_tx: Sender<(ConnectionHandle<I>, Bytes)>,
    connection_tx: Sender<ConnectionHandle<I>>,
    disconnection_tx: Sender<SocketAddr>,
    endpoint: Endpoint<I>,
    quic_endpoint: quinn::Endpoint,
) {
    let _ = tokio::spawn(async move {
        loop {
            match quinn_incoming.next().await {
                Some(quinn_conn) => match quinn_conn.await {
                    Ok(connection) => {
                        let (connection, connection_incoming) =
                            Connection::new(quic_endpoint.clone(), connection);

                        let peer_address = connection.remote_address();
                        let id = ConnId::generate(&peer_address);
                        let pool_handle = connection_pool
                            .insert(id, peer_address, connection.clone())
                            .await;
                        let connection = endpoint.wrap_connection(connection, pool_handle);
                        let _ = connection_tx.send(connection.clone()).await;
                        listen_for_incoming_messages(
                            connection,
                            connection_incoming,
                            message_tx.clone(),
                            disconnection_tx.clone(),
                        );
                    }
                    Err(err) => {
                        warn!("An incoming connection failed because of: {:?}", err);
                    }
                },
                None => {
                    trace!("quinn::Incoming::next() returned None. There will be no more incoming connections");
                    break;
                }
            }
        }
    });
}

pub(super) fn listen_for_incoming_messages<I: ConnId>(
    connection: ConnectionHandle<I>,
    mut connection_incoming: ConnectionIncoming,
    message_tx: Sender<(ConnectionHandle<I>, Bytes)>,
    disconnection_tx: Sender<SocketAddr>,
) {
    let _ = tokio::spawn(async move {
        let src = connection.remote_address();
        loop {
            match connection_incoming.next().await {
                Ok(Some(msg)) => {
                    let _ = message_tx.send((connection.clone(), msg)).await;
                }
                Ok(None) => {
                    break;
                }
                Err(error) => {
                    trace!("Issue on stream reading from {}: {:?}", src, error);
                    break;
                }
            }
        }

        connection.remover.remove().await;
        let _ = disconnection_tx.send(src).await;

        trace!("The connection to {} has terminated", src);
    });
}
