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
    error::{ConnectionError, RecvError, RpcError, SendError, SerializationError},
    wire_msg::WireMsg,
};
use crate::{Endpoint, RetryConfig};
use bytes::Bytes;
use futures::{future, stream::StreamExt};
use std::{fmt::Debug, net::SocketAddr, sync::Arc};
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::time::{timeout, Duration};
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
    quic_conn: quinn::Connection,
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
        quic_conn: quinn::Connection,
        default_retry_config: Arc<RetryConfig>,
        remover: ConnectionRemover<I>,
    ) -> Self {
        Self {
            quic_conn,
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
        self.quic_conn.remote_address()
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
        let (send_stream, recv_stream) = self.handle_error(self.quic_conn.open_bi().await).await?;
        let send_stream = SendStream::new(send_stream);
        send_stream.set_priority(priority);
        Ok((send_stream, RecvStream::new(recv_stream)))
    }

    /// Send message to peer using a uni-directional stream.
    /// Priority default is 0. Both lower and higher can be passed in.
    pub(crate) async fn send_uni(&self, msg: Bytes, priority: i32) -> Result<(), SendError> {
        let mut send_stream = self.handle_error(self.quic_conn.open_uni().await).await?;

        // quinn returns `UnknownStream` error if the stream does not exist. We ignore it, on the
        // basis that operations on the stream will fail instead (and the effect of setting priority
        // or not is only observable if the stream exists).
        let _ = send_stream.set_priority(priority);

        self.handle_error(send_msg(&mut send_stream, msg).await)
            .await?;

        // We try to make sure the stream is gracefully closed and the bytes get sent,
        // but if it was already closed (perhaps by the peer) then we
        // don't remove the connection from the pool.
        match send_stream.finish().await {
            Ok(()) | Err(quinn::WriteError::Stopped(_)) => Ok(()),
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

/// Stream to receive multiple messages
pub struct RecvStream {
    pub(crate) quinn_recv_stream: quinn::RecvStream,
}

impl RecvStream {
    pub(crate) fn new(quinn_recv_stream: quinn::RecvStream) -> Self {
        Self { quinn_recv_stream }
    }

    /// Read next user message from the stream
    pub async fn next(&mut self) -> Result<Bytes, RecvError> {
        match self.next_wire_msg().await? {
            Some(WireMsg::UserMsg(bytes)) => Ok(bytes),
            msg => Err(SerializationError::unexpected(msg).into()),
        }
    }

    /// Read next wire msg from the stream
    pub(crate) async fn next_wire_msg(&mut self) -> Result<Option<WireMsg>, RecvError> {
        read_bytes(&mut self.quinn_recv_stream).await
    }
}

impl Debug for RecvStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "RecvStream {{ .. }} ")
    }
}

/// Stream of outgoing messages
pub struct SendStream {
    pub(crate) quinn_send_stream: quinn::SendStream,
}

impl SendStream {
    pub(crate) fn new(quinn_send_stream: quinn::SendStream) -> Self {
        Self { quinn_send_stream }
    }

    /// Set the priority of the send stream
    ///
    /// Every send stream has an initial priority of 0. Locally buffered data from streams with
    /// higher priority will be transmitted before data from streams with lower priority. Changing
    /// the priority of a stream with pending data may only take effect after that data has been
    /// transmitted. Using many different priority levels per connection may have a negative
    /// impact on performance.
    pub fn set_priority(&self, priority: i32) {
        // quinn returns `UnknownStream` error if the stream does not exist. We ignore it, on the
        // basis that operations on the stream will fail instead (and the effect of setting priority
        // or not is only observable if the stream exists).
        let _ = self.quinn_send_stream.set_priority(priority);
    }

    /// Send a message using the stream created by the initiator
    pub async fn send_user_msg(&mut self, msg: Bytes) -> Result<(), SendError> {
        send_msg(&mut self.quinn_send_stream, msg).await
    }

    /// Send a wire message
    pub(crate) async fn send(&mut self, msg: WireMsg) -> Result<(), SendError> {
        msg.write_to_stream(&mut self.quinn_send_stream).await
    }

    /// Gracefully finish current stream
    pub async fn finish(mut self) -> Result<(), SendError> {
        self.quinn_send_stream.finish().await?;
        Ok(())
    }
}

impl Debug for SendStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "SendStream {{ .. }} ")
    }
}

// Helper to read the message's bytes from the provided stream
async fn read_bytes(recv: &mut quinn::RecvStream) -> Result<Option<WireMsg>, RecvError> {
    WireMsg::read_from_stream(recv).await
}

// Helper to send bytes to peer using the provided stream.
async fn send_msg(mut send_stream: &mut quinn::SendStream, msg: Bytes) -> Result<(), SendError> {
    let wire_msg = WireMsg::UserMsg(msg);
    wire_msg.write_to_stream(&mut send_stream).await?;
    Ok(())
}

pub(super) fn listen_for_incoming_connections<I: ConnId>(
    mut quinn_incoming: quinn::Incoming,
    connection_pool: ConnectionPool<I>,
    message_tx: Sender<(ConnectionHandle<I>, Bytes)>,
    connection_tx: Sender<ConnectionHandle<I>>,
    disconnection_tx: Sender<SocketAddr>,
    endpoint: Endpoint<I>,
) {
    let _ = tokio::spawn(async move {
        loop {
            match quinn_incoming.next().await {
                Some(quinn_conn) => match quinn_conn.await {
                    Ok(quinn::NewConnection {
                        connection,
                        uni_streams,
                        bi_streams,
                        ..
                    }) => {
                        let peer_address = connection.remote_address();
                        let id = ConnId::generate(&peer_address);
                        let pool_handle = connection_pool
                            .insert(id, peer_address, connection.clone())
                            .await;
                        let connection = endpoint.wrap_connection(connection, pool_handle);
                        let _ = connection_tx.send(connection.clone()).await;
                        listen_for_incoming_messages(
                            connection,
                            uni_streams,
                            bi_streams,
                            message_tx.clone(),
                            disconnection_tx.clone(),
                            endpoint.clone(),
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
    mut uni_streams: quinn::IncomingUniStreams,
    mut bi_streams: quinn::IncomingBiStreams,
    message_tx: Sender<(ConnectionHandle<I>, Bytes)>,
    disconnection_tx: Sender<SocketAddr>,
    endpoint: Endpoint<I>,
) {
    let src = connection.remote_address();
    let _ = tokio::spawn(async move {
        match future::try_join(
            read_on_uni_streams(&connection, &mut uni_streams, message_tx.clone()),
            read_on_bi_streams(&connection, &mut bi_streams, message_tx, &endpoint),
        )
        .await
        {
            Ok(_) => {
                connection.remover.remove().await;
                let _ = disconnection_tx.send(src).await;
            }
            Err(error) => {
                trace!("Issue on stream reading from: {:?} :: {:?}", src, error);
                connection.remover.remove().await;
                let _ = disconnection_tx.send(src).await;
            }
        }

        trace!("The connection to {:?} has been terminated.", src);
    });
}

// unify uni- and bi-stream errors
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
enum StreamError {
    Uni(#[from] RecvError),
    Bi(#[from] RpcError),
}

// Read messages sent by peer in an unidirectional stream.
async fn read_on_uni_streams<I: ConnId>(
    connection: &ConnectionHandle<I>,
    uni_streams: &mut quinn::IncomingUniStreams,
    message_tx: Sender<(ConnectionHandle<I>, Bytes)>,
) -> Result<(), StreamError> {
    let peer_addr = connection.remote_address();
    while let Some(result) = uni_streams.next().await {
        match result {
            Err(error @ quinn::ConnectionError::ConnectionClosed(_)) => {
                trace!("Connection closed by peer {:?}", peer_addr);
                return Err(StreamError::Uni(error.into()));
            }
            Err(error @ quinn::ConnectionError::ApplicationClosed(_)) => {
                trace!("Connection closed by peer {:?}.", peer_addr);
                return Err(StreamError::Uni(error.into()));
            }
            Err(err) => {
                warn!(
                    "Failed to read incoming message on uni-stream for peer {:?} with: {:?}",
                    peer_addr, err
                );
                return Err(StreamError::Uni(err.into()));
            }
            Ok(mut recv) => {
                while let Some(res) = read_bytes(&mut recv).await.transpose() {
                    match res {
                        Ok(WireMsg::UserMsg(bytes)) => {
                            trace!("bytes received fine from: {:?} ", peer_addr);
                            let _ = message_tx.send((connection.clone(), bytes)).await;
                        }
                        Ok(msg) => warn!("Unexpected message type: {:?}", msg),
                        Err(err) => {
                            warn!(
                                "Failed reading from a uni-stream for peer {:?} with: {:?}",
                                peer_addr, err
                            );
                            break;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

// Read messages sent by peer in a bidirectional stream.
async fn read_on_bi_streams<I: ConnId>(
    connection: &ConnectionHandle<I>,
    bi_streams: &mut quinn::IncomingBiStreams,
    message_tx: Sender<(ConnectionHandle<I>, Bytes)>,
    endpoint: &Endpoint<I>,
) -> Result<(), StreamError> {
    let peer_addr = connection.remote_address();
    while let Some(result) = bi_streams.next().await {
        match result {
            Err(error @ quinn::ConnectionError::ConnectionClosed(_)) => {
                trace!("Connection closed by peer {:?}", peer_addr);
                return Err(StreamError::Bi(error.into()));
            }
            Err(error @ quinn::ConnectionError::ApplicationClosed(_)) => {
                trace!("Connection closed by peer {:?}.", peer_addr);
                return Err(StreamError::Bi(error.into()));
            }
            Err(err) => {
                warn!(
                    "Failed to read incoming message on bi-stream for peer {:?} with: {:?}",
                    peer_addr, err
                );
                return Err(StreamError::Bi(err.into()));
            }
            Ok((mut send, mut recv)) => {
                while let Some(res) = read_bytes(&mut recv).await.transpose() {
                    match res {
                        Ok(WireMsg::UserMsg(bytes)) => {
                            let _ = message_tx.send((connection.clone(), bytes)).await;
                        }
                        Ok(WireMsg::EndpointEchoReq) => {
                            if let Err(error) = handle_endpoint_echo_req(peer_addr, &mut send).await
                            {
                                warn!(
                                    "Failed to handle Echo Request for peer {:?} with: {:?}",
                                    peer_addr, error
                                );

                                return Err(StreamError::Bi(error.into()));
                            }
                        }
                        Ok(WireMsg::EndpointVerificationReq(address_sent)) => {
                            if let Err(error) = handle_endpoint_verification_req(
                                peer_addr,
                                address_sent,
                                &mut send,
                                endpoint,
                            )
                            .await
                            {
                                warn!("Failed to handle Endpoint verification request for peer {:?} with: {:?}", peer_addr, error);

                                return Err(StreamError::Bi(error));
                            }
                        }
                        Ok(msg) => {
                            warn!(
                                "Unexpected message type from peer {:?}: {:?}",
                                peer_addr, msg
                            );
                        }
                        Err(err) => {
                            warn!(
                                "Failed reading from a bi-stream for peer {:?} with: {:?}",
                                peer_addr, err
                            );
                            break;
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

async fn handle_endpoint_echo_req(
    peer_addr: SocketAddr,
    send_stream: &mut quinn::SendStream,
) -> Result<(), SendError> {
    trace!("Received Echo Request from peer {:?}", peer_addr);
    let message = WireMsg::EndpointEchoResp(peer_addr);
    message.write_to_stream(send_stream).await?;
    trace!("Responded to Echo request from peer {:?}", peer_addr);
    Ok(())
}

async fn handle_endpoint_verification_req<I: ConnId>(
    peer_addr: SocketAddr,
    addr_sent: SocketAddr,
    send_stream: &mut quinn::SendStream,
    endpoint: &Endpoint<I>,
) -> Result<(), RpcError> {
    trace!(
        "Received Endpoint verification request {:?} from {:?}",
        addr_sent,
        peer_addr
    );
    // Verify if the peer's endpoint is reachable via EchoServiceReq
    let (mut temp_send, mut temp_recv) = endpoint.open_bidirectional_stream(&addr_sent, 0).await?;
    let message = WireMsg::EndpointEchoReq;
    message
        .write_to_stream(&mut temp_send.quinn_send_stream)
        .await?;
    let verified = matches!(
        timeout(
            Duration::from_secs(30),
            WireMsg::read_from_stream(&mut temp_recv.quinn_recv_stream)
        )
        .await,
        Ok(Ok(Some(WireMsg::EndpointEchoResp(_))))
    );

    let message = WireMsg::EndpointVerificationResp(verified);
    message.write_to_stream(send_stream).await?;
    trace!(
        "Responded to Endpoint verification request from {:?}",
        peer_addr
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{tests::new_endpoint, wire_msg::WireMsg};
    use color_eyre::eyre::{bail, Result};

    #[tokio::test(flavor = "multi_thread")]
    async fn echo_service() -> Result<()> {
        // Create Endpoint
        let (peer1, mut peer1_connections, _, _, _) = new_endpoint().await?;
        let peer1_addr = peer1.public_addr();

        let (peer2, _, _, _, _) = new_endpoint().await?;
        let peer2_addr = peer2.public_addr();

        let _ = peer2.connect_to(&peer1_addr).await?;

        if let Some(connection) = peer1_connections.next().await {
            assert_eq!(connection.remote_address(), peer2_addr);
        }

        let connection = peer1.connect_to(&peer2_addr).await?;
        let (mut send_stream, mut recv_stream) = connection.open_bi(0).await?;
        let message = WireMsg::EndpointEchoReq;
        message
            .write_to_stream(&mut send_stream.quinn_send_stream)
            .await?;
        let message = WireMsg::read_from_stream(&mut recv_stream.quinn_recv_stream).await?;
        if let Some(WireMsg::EndpointEchoResp(addr)) = message {
            assert_eq!(addr, peer1_addr);
        } else {
            bail!("Unexpected response to EchoService request");
        }
        Ok(())
    }
}
