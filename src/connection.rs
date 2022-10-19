//! A message-oriented API wrapping the underlying QUIC library (`quinn`).

use crate::{
    config::SERVER_NAME,
    error::{ConnectionError, RecvError, RpcError, SendError, StreamError},
    wire_msg::{UsrMsgBytes, WireMsg},
};
use futures::{
    future,
    stream::{self, StreamExt, TryStreamExt},
};
use std::{fmt, net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    sync::{mpsc, watch},
    time::timeout,
};
use tracing::{trace, warn};

// TODO: this seems arbitrary - it may need tuned or made configurable.
const INCOMING_MESSAGE_BUFFER_LEN: usize = 10_000;

// TODO: this seems arbitrary - it may need tuned or made configurable.
const ENDPOINT_VERIFICATION_TIMEOUT: Duration = Duration::from_secs(30);

// Error reason for closing a connection when triggered manually by qp2p apis
const QP2P_CLOSED_CONNECTION: &str = "The connection was closed intentionally by qp2p.";

type ResponseStream = SendStream;

/// The sending API for a connection.
#[derive(Clone)]
pub struct Connection {
    inner: quinn::Connection,

    // A reference to the 'alive' marker for the connection. This isn't read by `Connection`, but
    // must be held to keep background listeners alive until both halves of the connection are
    // dropped.
    _alive_tx: Arc<watch::Sender<()>>,
}

impl Connection {
    pub(crate) fn new(
        endpoint: quinn::Endpoint,
        connection: quinn::NewConnection,
    ) -> (Connection, ConnectionIncoming) {
        // this channel serves to keep the background message listener alive so long as one side of
        // the connection API is alive.
        let (alive_tx, alive_rx) = watch::channel(());
        let alive_tx = Arc::new(alive_tx);
        let peer_address = connection.connection.remote_address();
        let conn = Self {
            inner: connection.connection,
            _alive_tx: Arc::clone(&alive_tx),
        };
        let conn_id = conn.id();

        (
            conn,
            ConnectionIncoming::new(
                endpoint,
                conn_id,
                peer_address,
                connection.uni_streams,
                connection.bi_streams,
                alive_tx,
                alive_rx,
            ),
        )
    }

    /// A stable identifier for the connection.
    ///
    /// This ID will not change for the lifetime of the connection to a given ip.
    ///
    /// The ID pulls the internal conneciton id and concats with the SocketAddr of
    /// the peer. So this _should_ be unique per peer (without IP spoofing).
    ///
    pub fn id(&self) -> String {
        let remote_addr = self.remote_address();
        format!("{remote_addr}{}", self.inner.stable_id())
    }

    /// The address of the remote peer.
    pub fn remote_address(&self) -> SocketAddr {
        self.inner.remote_address()
    }

    /// Send a message to the peer.
    ///
    /// The message will be sent on a unidirectional QUIC stream, meaning the application is
    /// responsible for correlating any anticipated responses from incoming streams.
    ///
    /// The priority will be `0`.
    pub async fn send(&self, bytes: UsrMsgBytes) -> Result<(), SendError> {
        self.send_with(bytes, 0).await
    }

    /// Send a message to the peer using the given configuration.
    ///
    /// See [`send`](Self::send) if you want to send with the default configuration.
    pub async fn send_with(
        &self,
        user_msg_bytes: UsrMsgBytes,
        priority: i32,
    ) -> Result<(), SendError> {
        self.send_uni(user_msg_bytes, priority).await?;
        Ok(())
    }

    /// Open a unidirection stream to the peer.
    ///
    /// Messages sent over the stream will arrive at the peer in the order they were sent.
    pub async fn open_uni(&self) -> Result<SendStream, ConnectionError> {
        let send_stream = self.inner.open_uni().await?;
        Ok(SendStream::new(send_stream, self.id()))
    }

    /// Open a bidirectional stream to the peer.
    ///
    /// Bidirectional streams allow messages to be sent in both directions. This can be useful to
    /// automatically correlate response messages, for example.
    ///
    /// Messages sent over the stream will arrive at the peer in the order they were sent.
    pub async fn open_bi(&self) -> Result<(SendStream, RecvStream), ConnectionError> {
        let (send_stream, recv_stream) = self.inner.open_bi().await?;
        Ok((
            SendStream::new(send_stream, self.id()),
            RecvStream::new(recv_stream, self.id()),
        ))
    }

    /// Close the connection immediately.
    ///
    /// This is not a graceful close - pending operations will fail immediately with
    /// [`ConnectionError::Closed`]`(`[`Close::Local`]`)`, and data on unfinished streams is not
    /// guaranteed to be delivered.
    pub fn close(&self, reason: Option<String>) {
        let reason = reason.unwrap_or_else(|| QP2P_CLOSED_CONNECTION.to_string());
        self.inner.close(0u8.into(), &reason.into_bytes());
    }

    /// Opens a uni directional stream and sends message on this stream
    async fn send_uni(&self, user_msg_bytes: UsrMsgBytes, priority: i32) -> Result<(), SendError> {
        let mut send_stream = self.open_uni().await.map_err(SendError::ConnectionLost)?;
        send_stream.set_priority(priority);

        send_stream.send_user_msg(user_msg_bytes).await?;

        // We try to make sure the stream is gracefully closed and the bytes get sent, but if it
        // was already closed (perhaps by the peer) then we ignore the error.
        // TODO: we probably shouldn't ignore the error...
        send_stream.finish().await.or_else(|err| match err {
            SendError::StreamLost(StreamError::Stopped(_)) => Ok(()),
            _ => Err(err),
        })?;

        Ok(())
    }
}

impl fmt::Debug for Connection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Connection")
            .field("id", &self.id())
            .field("remote_address", &self.remote_address())
            .finish_non_exhaustive()
    }
}

/// Identifier for a stream within a particular connection
#[cfg_attr(feature = "arbitrary", derive(Arbitrary))]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct StreamId {
    stream_id: quinn::StreamId,
    conn_id: String,
}

impl fmt::Display for StreamId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let initiator = if self.stream_id.initiator().is_client() {
            "initiator"
        } else {
            "acceptor"
        };
        let dir = self.stream_id.dir();
        write!(
            f,
            "{initiator} {dir:?}directional stream {}@{}",
            self.stream_id.index(),
            self.conn_id
        )
    }
}

/// The sending API for a QUIC stream.
pub struct SendStream {
    conn_id: String,
    inner: quinn::SendStream,
}

impl SendStream {
    fn new(inner: quinn::SendStream, conn_id: String) -> Self {
        Self { conn_id, inner }
    }

    /// Get the identity of this stream
    pub fn id(&self) -> StreamId {
        StreamId {
            stream_id: self.inner.id(),
            conn_id: self.conn_id.clone(),
        }
    }

    /// Set the priority of the send stream.
    ///
    /// Every send stream has an initial priority of 0. Locally buffered data from streams with
    /// higher priority will be transmitted before data from streams with lower priority. Changing
    /// the priority of a stream with pending data may only take effect after that data has been
    /// transmitted. Using many different priority levels per connection may have a negative impact
    /// on performance.
    pub fn set_priority(&self, priority: i32) {
        // quinn returns `UnknownStream` error if the stream does not exist. We ignore it, on the
        // basis that operations on the stream will fail instead (and the effect of setting priority
        // or not is only observable if the stream exists).
        let _ = self.inner.set_priority(priority);
    }

    /// Send a message over the stream to the peer.
    ///
    /// Messages sent over the stream will arrive at the peer in the order they were sent.
    pub async fn send_user_msg(&mut self, user_msg_bytes: UsrMsgBytes) -> Result<(), SendError> {
        WireMsg::UserMsg(user_msg_bytes)
            .write_to_stream(&mut self.inner)
            .await
    }

    /// Shut down the send stream gracefully.
    ///
    /// The returned future will complete once the peer has acknowledged all sent data.
    pub async fn finish(&mut self) -> Result<(), SendError> {
        self.inner.finish().await?;
        Ok(())
    }

    pub(crate) async fn send_wire_msg(&mut self, msg: WireMsg) -> Result<(), SendError> {
        msg.write_to_stream(&mut self.inner).await
    }
}

impl fmt::Debug for SendStream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("SendStream").finish_non_exhaustive()
    }
}

/// The receiving API for a bidirectional QUIC stream.
pub struct RecvStream {
    conn_id: String,
    inner: quinn::RecvStream,
}

impl RecvStream {
    fn new(inner: quinn::RecvStream, conn_id: String) -> Self {
        Self { conn_id, inner }
    }

    /// Get the identity of this stream
    pub fn id(&self) -> StreamId {
        StreamId {
            stream_id: self.inner.id(),
            conn_id: self.conn_id.clone(),
        }
    }

    /// Get the next message sent by the peer over this stream.
    pub async fn next(&mut self) -> Result<Option<UsrMsgBytes>, RecvError> {
        match self.next_wire_msg().await? {
            Some(WireMsg::UserMsg(msg)) => Ok(Some(msg)),
            Some(msg) => Err(RecvError::UnexpectedMsgReceived(msg.to_string())),
            None => Ok(None),
        }
    }

    pub(crate) async fn next_wire_msg(&mut self) -> Result<Option<WireMsg>, RecvError> {
        WireMsg::read_from_stream(&mut self.inner).await
    }
}

impl fmt::Debug for RecvStream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("RecvStream").finish_non_exhaustive()
    }
}

/// The receiving API for a connection.
#[derive(Debug)]
pub struct ConnectionIncoming {
    message_rx: mpsc::Receiver<Result<(UsrMsgBytes, Option<ResponseStream>), RecvError>>,
    _alive_tx: Arc<watch::Sender<()>>,
}

impl ConnectionIncoming {
    fn new(
        endpoint: quinn::Endpoint,
        conn_id: String,
        peer_addr: SocketAddr,
        uni_streams: quinn::IncomingUniStreams,
        bi_streams: quinn::IncomingBiStreams,
        alive_tx: Arc<watch::Sender<()>>,
        alive_rx: watch::Receiver<()>,
    ) -> Self {
        let (message_tx, message_rx) = mpsc::channel(INCOMING_MESSAGE_BUFFER_LEN);

        // offload the actual message handling to a background task - the task will exit when
        // `alive_tx` is dropped, which would be when both sides of the connection are dropped.
        start_message_listeners(
            endpoint,
            conn_id,
            peer_addr,
            uni_streams,
            bi_streams,
            alive_rx,
            message_tx,
        );

        Self {
            message_rx,
            _alive_tx: alive_tx,
        }
    }

    /// Get the next message sent by the peer, over any stream.
    pub async fn next(&mut self) -> Result<Option<UsrMsgBytes>, RecvError> {
        if let Some((bytes, _opt)) = self.next_with_stream().await? {
            Ok(Some(bytes))
        } else {
            Ok(None)
        }
    }

    /// Get the next message sent by the peer, over any stream along with the stream to respond with.
    pub async fn next_with_stream(
        &mut self,
    ) -> Result<Option<(UsrMsgBytes, Option<ResponseStream>)>, RecvError> {
        self.message_rx.recv().await.transpose()
    }
}

// Start listeners in background tokio tasks. These tasks will run until they terminate, which would
// be when the connection terminates, or all connection handles are dropped.
//
// `alive_tx` is used to detect when all connection handles are dropped.
// `message_tx` is used to exfiltrate messages and stream errors.
fn start_message_listeners(
    endpoint: quinn::Endpoint,
    conn_id: String,
    peer_addr: SocketAddr,
    uni_streams: quinn::IncomingUniStreams,
    bi_streams: quinn::IncomingBiStreams,
    alive_rx: watch::Receiver<()>,
    message_tx: mpsc::Sender<Result<(UsrMsgBytes, Option<ResponseStream>), RecvError>>,
) {
    let _ = tokio::spawn(listen_on_uni_streams(
        peer_addr,
        uni_streams,
        alive_rx.clone(),
        message_tx.clone(),
    ));

    let _ = tokio::spawn(listen_on_bi_streams(
        endpoint, conn_id, peer_addr, bi_streams, alive_rx, message_tx,
    ));
}

async fn listen_on_uni_streams(
    peer_addr: SocketAddr,
    uni_streams: quinn::IncomingUniStreams,
    mut alive_rx: watch::Receiver<()>,
    message_tx: mpsc::Sender<Result<(UsrMsgBytes, Option<ResponseStream>), RecvError>>,
) {
    trace!(
        "Started listener for incoming uni-streams from {}",
        peer_addr
    );

    let mut uni_messages = Box::pin(
        uni_streams
            .map_ok(|recv_stream| {
                trace!("Handling incoming uni-stream from {}", peer_addr);

                stream::try_unfold(recv_stream, |mut recv_stream| async move {
                    WireMsg::read_from_stream(&mut recv_stream)
                        .await
                        .and_then(|msg| match msg {
                            Some(WireMsg::UserMsg(msg)) => Ok(Some((msg, recv_stream))),
                            Some(other_msg) => {
                                Err(RecvError::UnexpectedMsgReceived(other_msg.to_string()))
                            }
                            None => Ok(None),
                        })
                })
            })
            .try_flatten(),
    );

    // it's a shame to allocate, but there are `Pin` errors otherwise – and we should only be doing
    // this once (per connection).
    let mut alive = Box::pin(alive_rx.changed());

    while let Some(result) = {
        match future::select(uni_messages.next(), &mut alive).await {
            future::Either::Left((result, _)) => result,
            future::Either::Right((Ok(_), pending_message)) => {
                // we don't expect to actually send a value over the alive channel, so just ignore
                pending_message.await
            }
            future::Either::Right((Err(_), _)) => {
                // the connection has been dropped
                // TODO: should we just drop pending messages here? if not, how long do we wait?
                trace!(
                    "Stopped listener for incoming uni-streams from {}: connection handles dropped",
                    peer_addr
                );
                None
            }
        }
    } {
        let mut break_ = false;

        if let Err(RecvError::ConnectionLost(_)) = &result {
            // if the connection is lost, we should stop processing (after sending the error)
            break_ = true;
        }

        if message_tx.send(result.map(|b| (b, None))).await.is_err() {
            // if we can't send the result, the receiving end is closed so we should stop processing
            break_ = true;
        }

        if break_ {
            break;
        }
    }
    trace!(
        "Stopped listener for incoming uni-streams from {}: stream finished",
        peer_addr
    );
}

async fn listen_on_bi_streams(
    endpoint: quinn::Endpoint,
    conn_id: String,
    peer_addr: SocketAddr,
    bi_streams: quinn::IncomingBiStreams,
    mut alive_rx: watch::Receiver<()>,
    message_tx: mpsc::Sender<Result<(UsrMsgBytes, Option<ResponseStream>), RecvError>>,
) {
    trace!("Started listener for incoming bi-streams from {peer_addr}");
    let streaming = bi_streams.try_for_each_concurrent(None, |(send_stream, mut recv_stream)| {
        let endpoint = &endpoint;
        let message_tx = &message_tx;
        let conn_id = &conn_id;
        async move {
            trace!("Handling incoming bi-stream from {peer_addr}");
            match WireMsg::read_from_stream(&mut recv_stream).await {
                Err(error) => {
                    if let Err(error) = message_tx.send(Err(error)).await {
                        // if we can't send the result, the receiving end is closed so we should stop
                        trace!("Receiver gone, dropping error: {error:?}");
                    }
                }
                Ok(None) => {}
                Ok(Some(WireMsg::UserMsg((header, dst, payload)))) => {
                    if let Err(msg) = message_tx
                        .send(Ok((
                            (header, dst, payload),
                            Some(SendStream::new(send_stream, conn_id.clone())),
                        )))
                        .await
                    {
                        // if we can't send the result, the receiving end is closed so we should stop
                        trace!("Receiver gone, dropping message: {msg:?}");
                    }
                }
                Ok(Some(WireMsg::EndpointEchoReq)) => {
                    if let Err(error) = handle_endpoint_echo(send_stream, peer_addr).await {
                        // TODO: consider more carefully how to handle this
                        warn!("Error handling endpoint echo request: {error:?}");
                    }
                }
                Ok(Some(WireMsg::EndpointVerificationReq(addr))) => {
                    if let Err(error) =
                        handle_endpoint_verification(endpoint, send_stream, addr).await
                    {
                        // TODO: consider more carefully how to handle this
                        warn!("Error handling endpoint verification request: {error:?}");
                    }
                }
                Ok(Some(other_msg)) => {
                    // TODO: consider more carefully how to handle this
                    warn!("Unexpected type of message received on bi-stream: {other_msg}");
                }
            }

            Ok(())
        }
    });

    // it's a shame to allocate, but there are `Pin` errors otherwise – and we should only be doing
    // this once.
    let mut alive = Box::pin(alive_rx.changed());

    match future::select(streaming, &mut alive).await {
        future::Either::Left((Ok(()), _)) => {
            trace!("Stopped listener for incoming bi-streams from {peer_addr}: stream ended");
        }
        future::Either::Left((Err(error), _)) => {
            // A connection error occurred on bi_streams, we don't propagate anything here as we
            // expect propagation to be handled in listen_on_uni_streams.
            warn!(
                "Stopped listener for incoming bi-streams from {peer_addr} due to error: {error:?}"
            );
        }
        future::Either::Right((_, _)) => {
            // the connection was closed
            // TODO: should we just drop pending messages here? if not, how long do we wait?
            trace!("Stopped listener for incoming bi-streams from {peer_addr}: connection handles dropped");
        }
    }
}

async fn handle_endpoint_echo(
    mut send_stream: quinn::SendStream,
    peer_addr: SocketAddr,
) -> Result<(), SendError> {
    trace!("Replying to EndpointEchoReq from {peer_addr}");
    WireMsg::EndpointEchoResp(peer_addr)
        .write_to_stream(&mut send_stream)
        .await
}

async fn handle_endpoint_verification(
    endpoint: &quinn::Endpoint,
    mut verif_response_stream: quinn::SendStream,
    addr: SocketAddr,
) -> Result<(), SendError> {
    trace!("Performing endpoint verification for {addr}");

    let verify = async {
        trace!("EndpointVerificationReq: opening new connection to {addr}");
        let connection = endpoint
            .connect(addr, SERVER_NAME)
            .map_err(ConnectionError::from)?
            .await?;

        let (mut send_stream, mut recv_stream) = connection.connection.open_bi().await?;
        trace!(
            "EndpointVerificationReq: sending EndpointEchoReq to {addr} over connection {}",
            connection.connection.stable_id()
        );
        WireMsg::EndpointEchoReq
            .write_to_stream(&mut send_stream)
            .await?;

        match WireMsg::read_from_stream(&mut recv_stream).await? {
            Some(WireMsg::EndpointEchoResp(_)) => {
                trace!("EndpointVerificationReq: Received EndpointEchoResp from {addr}");
                Ok(())
            }
            msg => Err(RpcError::EchoResponseMissing {
                peer: addr,
                response: msg.map(|m| m.to_string()),
            }),
        }
    };

    let verified: Result<_, RpcError> = timeout(ENDPOINT_VERIFICATION_TIMEOUT, verify)
        .await
        .unwrap_or_else(|error| Err(error.into()));

    if let Err(error) = &verified {
        warn!("Endpoint verification for {addr} failed: {error:?}");
    }

    WireMsg::EndpointVerificationResp(verified.is_ok())
        .write_to_stream(&mut verif_response_stream)
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::Connection;
    use crate::{
        config::{Config, InternalConfig, SERVER_NAME},
        error::{ConnectionError, RecvError, SendError},
        tests::local_addr,
        wire_msg::WireMsg,
    };
    use bytes::Bytes;
    use color_eyre::eyre::{bail, Result};
    use futures::{StreamExt, TryStreamExt};
    use quinn::Endpoint as QuinnEndpoint;
    use std::time::Duration;

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn basic_usage() -> Result<()> {
        let config = InternalConfig::try_from_config(Default::default())?;

        let (mut peer1, _peer1_incoming) =
            QuinnEndpoint::server(config.server.clone(), local_addr())?;
        peer1.set_default_client_config(config.client);

        let (peer2, peer2_incoming) = QuinnEndpoint::server(config.server.clone(), local_addr())?;

        {
            let (p1_tx, mut p1_rx) = Connection::new(
                peer1.clone(),
                peer1.connect(peer2.local_addr()?, SERVER_NAME)?.await?,
            );

            let (p2_tx, mut p2_rx) =
                if let Some(connection) = timeout(peer2_incoming.then(|c| c).try_next()).await?? {
                    Connection::new(peer2.clone(), connection)
                } else {
                    bail!("did not receive incoming connection when one was expected");
                };

            p1_tx
                .open_uni()
                .await?
                .send_user_msg((Bytes::new(), Bytes::new(), Bytes::from_static(b"hello")))
                .await?;

            if let Some((_, _, msg)) = timeout(p2_rx.next()).await?? {
                assert_eq!(&msg[..], b"hello");
            } else {
                bail!("did not receive message when one was expected");
            }

            p2_tx
                .open_uni()
                .await?
                .send_user_msg((Bytes::new(), Bytes::new(), Bytes::from_static(b"world")))
                .await?;

            if let Some((_, _, msg)) = timeout(p1_rx.next()).await?? {
                assert_eq!(&msg[..], b"world");
            } else {
                bail!("did not receive message when one was expected");
            }
        }

        // check the connections were shutdown on drop
        timeout(peer1.wait_idle()).await?;
        timeout(peer2.wait_idle()).await?;

        Ok(())
    }

    #[tokio::test]
    async fn connection_loss() -> Result<()> {
        let config = InternalConfig::try_from_config(Config {
            // set a very low idle timeout
            idle_timeout: Some(Duration::from_secs(1)),
            ..Default::default()
        })?;

        let (mut peer1, _peer1_incoming) =
            QuinnEndpoint::server(config.server.clone(), local_addr())?;
        peer1.set_default_client_config(config.client);

        let (peer2, peer2_incoming) = QuinnEndpoint::server(config.server.clone(), local_addr())?;

        // open a connection between the two peers
        let (p1_tx, _) = Connection::new(
            peer1.clone(),
            peer1.connect(peer2.local_addr()?, SERVER_NAME)?.await?,
        );

        let (_, mut p2_rx) =
            if let Some(connection) = timeout(peer2_incoming.then(|c| c).try_next()).await?? {
                Connection::new(peer2.clone(), connection)
            } else {
                bail!("did not receive incoming connection when one was expected");
            };

        // let 2 * idle timeout pass
        tokio::time::sleep(Duration::from_secs(2)).await;

        // trying to send a message should fail with an error
        match p1_tx
            .send((Bytes::new(), Bytes::new(), b"hello"[..].into()))
            .await
        {
            Err(SendError::ConnectionLost(ConnectionError::TimedOut)) => {}
            res => bail!("unexpected send result: {:?}", res),
        }

        // trying to receive should NOT return an error
        match p2_rx.next().await {
            Err(RecvError::ConnectionLost(ConnectionError::TimedOut)) => {}
            res => bail!("unexpected recv result: {:?}", res),
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_endpoint_echo() -> Result<()> {
        let config = InternalConfig::try_from_config(Config::default())?;

        let (mut peer1, _peer1_incoming) =
            QuinnEndpoint::server(config.server.clone(), local_addr())?;
        peer1.set_default_client_config(config.client);

        let (peer2, peer2_incoming) = QuinnEndpoint::server(config.server.clone(), local_addr())?;

        {
            let (p1_tx, _) = Connection::new(
                peer1.clone(),
                peer1.connect(peer2.local_addr()?, SERVER_NAME)?.await?,
            );

            // we need to accept the connection on p2, or the message won't be processed
            let _p2_handle =
                if let Some(connection) = timeout(peer2_incoming.then(|c| c).try_next()).await?? {
                    Connection::new(peer2.clone(), connection)
                } else {
                    bail!("did not receive incoming connection when one was expected");
                };

            let (mut send_stream, mut recv_stream) = p1_tx.open_bi().await?;
            send_stream.send_wire_msg(WireMsg::EndpointEchoReq).await?;

            if let Some(msg) = timeout(recv_stream.next_wire_msg()).await?? {
                if let WireMsg::EndpointEchoResp(addr) = msg {
                    assert_eq!(addr, peer1.local_addr()?);
                } else {
                    bail!(
                        "received unexpected message when EndpointEchoResp was expected: {:?}",
                        msg
                    );
                }
            } else {
                bail!("did not receive incoming message when one was expected");
            }
        }

        // check the connections were shutdown on drop
        timeout(peer1.wait_idle()).await?;
        timeout(peer2.wait_idle()).await?;

        Ok(())
    }

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn endpoint_verification() -> Result<()> {
        let config = InternalConfig::try_from_config(Default::default())?;

        let (mut peer1, peer1_incoming) =
            QuinnEndpoint::server(config.server.clone(), local_addr())?;
        peer1.set_default_client_config(config.client.clone());

        let (mut peer2, peer2_incoming) =
            QuinnEndpoint::server(config.server.clone(), local_addr())?;
        peer2.set_default_client_config(config.client);

        {
            let (p1_tx, _) = Connection::new(
                peer1.clone(),
                peer1.connect(peer2.local_addr()?, SERVER_NAME)?.await?,
            );

            // we need to accept the connection on p2, or the message won't be processed
            let _p2_handle =
                if let Some(connection) = timeout(peer2_incoming.then(|c| c).try_next()).await?? {
                    Connection::new(peer2.clone(), connection)
                } else {
                    bail!("did not receive incoming connection when one was expected");
                };

            let (mut send_stream, mut recv_stream) = p1_tx.open_bi().await?;
            send_stream
                .send_wire_msg(WireMsg::EndpointVerificationReq(peer1.local_addr()?))
                .await?;

            // we need to accept the connection on p1, or the message won't be processed
            let _p1_handle =
                if let Some(connection) = timeout(peer1_incoming.then(|c| c).try_next()).await?? {
                    Connection::new(peer1.clone(), connection)
                } else {
                    bail!("did not receive incoming connection when one was expected");
                };

            if let Some(msg) = timeout(recv_stream.next_wire_msg()).await?? {
                if let WireMsg::EndpointVerificationResp(true) = msg {
                } else {
                    bail!(
                        "received unexpected message when EndpointVerificationResp(true) was expected: {:?}",
                        msg
                    );
                }
            } else {
                bail!("did not receive incoming message when one was expected");
            }

            // only one msg per bi-stream is supported, let's create a new bi-stream for this test
            let (mut send_stream, mut recv_stream) = p1_tx.open_bi().await?;
            send_stream
                .send_wire_msg(WireMsg::EndpointVerificationReq(local_addr()))
                .await?;

            if let Some(msg) = timeout(recv_stream.next_wire_msg()).await?? {
                if let WireMsg::EndpointVerificationResp(false) = msg {
                } else {
                    bail!(
                        "received unexpected message when EndpointVerificationResp(false) was expected: {:?}",
                        msg
                    );
                }
            } else {
                bail!("did not receive incoming message when one was expected");
            }
        }

        // check the connections were shutdown on drop
        timeout(peer1.wait_idle()).await?;
        timeout(peer2.wait_idle()).await?;

        Ok(())
    }

    async fn timeout<F: std::future::Future>(
        f: F,
    ) -> Result<F::Output, tokio::time::error::Elapsed> {
        tokio::time::timeout(Duration::from_millis(500), f).await
    }
}
