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
    error::{ConnectionError, RecvError, SendError},
};
use crate::{
    connection::{Connection, ConnectionIncoming, RecvStream, SendStream},
    Endpoint, RetryConfig,
};
use bytes::Bytes;
use futures::stream::StreamExt;
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::mpsc::Sender;
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
        self.handle_error(self.inner.send(msg).await).await
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
        self.handle_error(self.inner.send_with(msg, priority, retry_config).await)
            .await
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

    async fn handle_error<T, E>(&self, result: Result<T, E>) -> Result<T, E> {
        if result.is_err() {
            self.remover.remove().await
        }

        result
    }
}

/// The receiving API for a connection.
pub struct ConnectionIncomingHandle<I: ConnId> {
    inner: ConnectionIncoming,
    remover: ConnectionRemover<I>,
}

impl<I: ConnId> ConnectionIncomingHandle<I> {
    pub(crate) fn new(inner: ConnectionIncoming, remover: ConnectionRemover<I>) -> Self {
        Self { inner, remover }
    }

    /// Get the next message sent by the peer, over any stream.
    pub async fn next(&mut self) -> Result<Option<Bytes>, RecvError> {
        let result = self.inner.next().await;

        if let Err(RecvError::ConnectionLost(_)) = &result {
            self.remover.remove().await;
        }

        result
    }
}

pub(super) fn listen_for_incoming_connections<I: ConnId>(
    mut quinn_incoming: quinn::Incoming,
    connection_pool: ConnectionPool<I>,
    connection_tx: Sender<(ConnectionHandle<I>, ConnectionIncomingHandle<I>)>,
    endpoint: Endpoint<I>,
    quic_endpoint: quinn::Endpoint,
    retry_config: Arc<RetryConfig>,
) {
    let _ = tokio::spawn(async move {
        loop {
            match quinn_incoming.next().await {
                Some(quinn_conn) => match quinn_conn.await {
                    Ok(connection) => {
                        let (connection, connection_incoming) = Connection::new(
                            quic_endpoint.clone(),
                            Some(retry_config.clone()),
                            connection,
                        );

                        let peer_address = connection.remote_address();
                        let id = ConnId::generate(&peer_address);
                        let pool_handle = connection_pool
                            .insert(id, peer_address, connection.clone())
                            .await;
                        let connection = endpoint.wrap_connection(connection, pool_handle.clone());
                        let connection_incoming =
                            ConnectionIncomingHandle::new(connection_incoming, pool_handle);
                        let _ = connection_tx.send((connection, connection_incoming)).await;
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
