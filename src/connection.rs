//! A message-oriented API wrapping the underlying QUIC library (`quinn`).

use crate::{
    config::SERVER_NAME,
    error::{ConnectionError, RecvError, RpcError, SendError, StreamError},
    wire_msg::{UsrMsgBytes, WireMsg},
};
use quinn::VarInt;
use std::{fmt, net::SocketAddr, time::Duration};
use tokio::{
    sync::mpsc::{Receiver, Sender},
    time::timeout,
};
use tracing::{trace, warn};

// TODO: this seems arbitrary - it may need tuned or made configurable.
const INCOMING_MESSAGE_BUFFER_LEN: usize = 10_000;

// TODO: this seems arbitrary - it may need tuned or made configurable.
const ENDPOINT_VERIFICATION_TIMEOUT: Duration = Duration::from_secs(30);

// Error reason for closing a connection when triggered manually by qp2p apis
const QP2P_CLOSED_CONNECTION: &str = "The connection was closed intentionally by qp2p.";

type IncomingMsg = Result<(UsrMsgBytes, Option<SendStream>), RecvError>;

/// The sending API for a connection.
pub struct Connection {
    inner: quinn::Connection,
}
impl Drop for Connection {
    fn drop(&mut self) {
        warn!(
            "Connection handle dropped, thus closing it, conn_id={}",
            self.id()
        );
        self.inner.close(VarInt::from_u32(0), b"lost interest");
    }
}

impl Connection {
    pub(crate) fn new(
        connection: quinn::Connection,
        endpoint: quinn::Endpoint,
    ) -> (Connection, ConnectionIncoming) {
        let (tx, rx) = tokio::sync::mpsc::channel(INCOMING_MESSAGE_BUFFER_LEN);
        listen_on_uni_streams(connection.clone(), tx.clone());
        listen_on_bi_streams(connection.clone(), endpoint, tx);

        (Self { inner: connection }, ConnectionIncoming(rx))
    }

    /// Returns `Some(...)` if the connection is closed.
    pub fn close_reason(&self) -> Option<ConnectionError> {
        self.inner.close_reason().map(|e| e.into())
    }

    /// A stable identifier for the connection.
    ///
    /// This ID will not change for the lifetime of the connection to a given ip.
    ///
    /// The ID pulls the internal conneciton id and concats with the SocketAddr of
    /// the peer. So this _should_ be unique per peer (without IP spoofing).
    ///
    pub fn id(&self) -> String {
        build_conn_id(&self.inner)
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
    pub async fn send(&self, user_msg_bytes: UsrMsgBytes) -> Result<(), SendError> {
        self.send_with(user_msg_bytes, 0).await
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
        let conn_id = self.id();
        Ok((
            SendStream::new(send_stream, conn_id.clone()),
            RecvStream::new(recv_stream, conn_id),
        ))
    }

    /// Close the connection immediately.
    ///
    /// This is not a graceful close - pending operations will fail immediately with
    /// [`ConnectionError::Closed`]`(`[`Close::Local`]`)`, and data on unfinished streams is not
    /// guaranteed to be delivered.
    pub fn close(&self, reason: Option<String>) {
        let reason = reason.unwrap_or_else(|| QP2P_CLOSED_CONNECTION.to_string());
        warn!("Closing connection witn conn_id={}", self.id());
        self.inner.close(0u8.into(), &reason.into_bytes());
    }

    /// Opens a uni-directional stream and sends message on it using the given priority.
    pub async fn send_with(
        &self,
        user_msg_bytes: UsrMsgBytes,
        priority: i32,
    ) -> Result<(), SendError> {
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

// Helper to build a connection identifier string
fn build_conn_id(conn: &quinn::Connection) -> String {
    format!("{}{}", conn.remote_address(), conn.stable_id())
}

fn listen_on_uni_streams(connection: quinn::Connection, tx: Sender<IncomingMsg>) {
    let conn_id = build_conn_id(&connection);

    let _ = tokio::spawn(async move {
        trace!("Connection {conn_id}: listening for incoming uni-streams");

        loop {
            // Wait for an incoming stream.
            let uni = connection.accept_uni().await.map_err(ConnectionError::from);
            let mut recv = match uni {
                Ok(recv) => recv,
                Err(err) => {
                    // In case of a connection error, there is not much we can do.
                    trace!(
                        "Connection {conn_id}: failure when awaiting incoming uni-streams: {err:?}"
                    );
                    // WARNING: This might block!
                    let _ = tx.send(Err(RecvError::ConnectionLost(err))).await;
                    break;
                }
            };
            trace!("Connection {conn_id}: incoming uni-stream accepted");

            let tx = tx.clone();

            // Make sure we are able to process multiple streams in parallel.
            let _ = tokio::spawn(async move {
                let msg = WireMsg::read_from_stream(&mut recv).await;
                let msg = match msg {
                    Ok(msg) => match msg {
                        WireMsg::UserMsg(msg) => Ok(msg),
                        _ => Err(RecvError::UnexpectedMsgReceived(msg.to_string())),
                    },
                    Err(err) => Err(err),
                };

                // Send away the msg or error
                let _ = tx.send(msg.map(|r| (r, None))).await;
            });
        }

        trace!("Connection {conn_id}: stopped listening for uni-streams");
    });
}

#[allow(clippy::type_complexity)]
fn listen_on_bi_streams(
    connection: quinn::Connection,
    endpoint: quinn::Endpoint,
    tx: Sender<IncomingMsg>,
) {
    let conn_id = build_conn_id(&connection);

    let _ = tokio::spawn(async move {
        trace!("Connection {conn_id}: listening for incoming bi-streams");

        loop {
            // Wait for an incoming stream.
            let bi = connection.accept_bi().await.map_err(ConnectionError::from);
            let (send, mut recv) = match bi {
                Ok(recv) => recv,
                Err(err) => {
                    // In case of a connection error, there is not much we can do.
                    trace!(
                        "Connection {conn_id}: failure when awaiting incoming bi-streams: {err:?}"
                    );
                    // WARNING: This might block!
                    let _ = tx.send(Err(RecvError::ConnectionLost(err))).await;
                    break;
                }
            };
            trace!("Connection {conn_id}: incoming bi-stream accepted");

            let tx = tx.clone();
            let endpoint = endpoint.clone();
            let addr = connection.remote_address();
            let conn_id = conn_id.clone();

            // Make sure we are able to process multiple streams in parallel.
            let _ = tokio::spawn(async move {
                let msg = WireMsg::read_from_stream(&mut recv).await;
                let msg = match msg {
                    Ok(WireMsg::UserMsg(msg)) => Ok(msg),
                    Ok(WireMsg::EndpointEchoReq) => {
                        if let Err(error) = handle_endpoint_echo(send, addr).await {
                            // TODO: consider more carefully how to handle this
                            warn!("Error handling endpoint echo request on conn_id {conn_id}: {error}");
                        }
                        return;
                    }
                    Ok(WireMsg::EndpointVerificationReq(addr)) => {
                        if let Err(error) =
                            handle_endpoint_verification(&endpoint, send, addr).await
                        {
                            // TODO: consider more carefully how to handle this
                            warn!("Error handling endpoint verification request on conn_id {conn_id}: {error}");
                        }
                        return;
                    }
                    // We do not expect other types.
                    Ok(msg) => Err(RecvError::UnexpectedMsgReceived(msg.to_string())),
                    Err(err) => Err(err),
                };

                // Pass the stream, so it can be used to respond to the user message.
                let msg = msg.map(|msg| (msg, Some(SendStream::new(send, conn_id.clone()))));
                // Send away the msg or error
                let _ = tx.send(msg).await;
                trace!("Incoming new msg on conn_id={conn_id} sent to user in upper layer");
            });
        }

        trace!("Connection {conn_id}: stopped listening for bi-streams");
    });
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

///
#[derive(Debug)]
pub struct ConnectionIncoming(Receiver<IncomingMsg>);
impl ConnectionIncoming {
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
    ) -> Result<Option<(UsrMsgBytes, Option<SendStream>)>, RecvError> {
        self.0.recv().await.transpose()
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

    /// Parse the message sent by the peer over this stream.
    pub async fn read(&mut self) -> Result<UsrMsgBytes, RecvError> {
        match self.read_wire_msg().await? {
            WireMsg::UserMsg(msg) => Ok(msg),
            msg => Err(RecvError::UnexpectedMsgReceived(msg.to_string())),
        }
    }

    pub(crate) async fn read_wire_msg(&mut self) -> Result<WireMsg, RecvError> {
        WireMsg::read_from_stream(&mut self.inner).await
    }
}

impl fmt::Debug for RecvStream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("RecvStream").finish_non_exhaustive()
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
    mut send_stream: quinn::SendStream,
    addr: SocketAddr,
) -> Result<(), SendError> {
    trace!("Performing endpoint verification for {addr}");

    let verify = async {
        trace!("EndpointVerificationReq: opening new connection to {addr}");
        let connection = endpoint
            .connect(addr, SERVER_NAME)
            .map_err(ConnectionError::from)?
            .await?;

        let (mut send_stream, mut recv_stream) = connection.open_bi().await?;
        trace!(
            "EndpointVerificationReq: sending EndpointEchoReq to {addr} over connection {}",
            connection.stable_id()
        );
        WireMsg::EndpointEchoReq
            .write_to_stream(&mut send_stream)
            .await?;

        match WireMsg::read_from_stream(&mut recv_stream).await? {
            WireMsg::EndpointEchoResp(_) => {
                trace!("EndpointVerificationReq: Received EndpointEchoResp from {addr}");
                Ok(())
            }
            msg => Err(RpcError::EchoResponseMissing {
                peer: addr,
                response: Some(msg.to_string()),
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
        .write_to_stream(&mut send_stream)
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::Connection;
    use crate::{
        config::{Config, InternalConfig, SERVER_NAME},
        error::{ConnectionError, SendError},
        tests::local_addr,
        wire_msg::WireMsg,
    };
    use bytes::Bytes;
    use color_eyre::eyre::{bail, Result};
    use futures::future::OptionFuture;
    use std::time::Duration;

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn basic_usage() -> Result<()> {
        let config = InternalConfig::try_from_config(Default::default())?;

        let mut peer1 = quinn::Endpoint::server(config.server.clone(), local_addr())?;
        peer1.set_default_client_config(config.client);

        let peer2 = quinn::Endpoint::server(config.server.clone(), local_addr())?;

        {
            let (p1_conn, mut p1_incoming) = Connection::new(
                peer1.connect(peer2.local_addr()?, SERVER_NAME)?.await?,
                peer1.clone(),
            );

            let (p2_conn, mut p2_incoming) = if let Some(connection) =
                timeout(OptionFuture::from(peer2.accept().await))
                    .await?
                    .and_then(|c| c.ok())
            {
                Connection::new(connection, peer2.clone())
            } else {
                bail!("did not receive incoming connection when one was expected");
            };

            p1_conn
                .open_uni()
                .await?
                .send_user_msg((Bytes::new(), Bytes::new(), Bytes::from_static(b"hello")))
                .await?;

            if let Ok(Some((_, _, msg))) = timeout(p2_incoming.next()).await? {
                assert_eq!(&msg[..], b"hello");
            } else {
                bail!("did not receive message when one was expected");
            }

            p2_conn
                .open_uni()
                .await?
                .send_user_msg((Bytes::new(), Bytes::new(), Bytes::from_static(b"world")))
                .await?;

            if let Ok(Some((_, _, msg))) = timeout(p1_incoming.next()).await? {
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

        let mut peer1 = quinn::Endpoint::server(config.server.clone(), local_addr())?;
        peer1.set_default_client_config(config.client);

        let peer2 = quinn::Endpoint::server(config.server.clone(), local_addr())?;

        // open a connection between the two peers
        let (p1_conn, _) = Connection::new(
            peer1.connect(peer2.local_addr()?, SERVER_NAME)?.await?,
            peer1.clone(),
        );

        let (_p2_conn, mut p2_incoming) = if let Some(connection) =
            timeout(OptionFuture::from(peer2.accept().await))
                .await?
                .and_then(|c| c.ok())
        {
            Connection::new(connection, peer2.clone())
        } else {
            bail!("did not receive incoming connection when one was expected");
        };

        // let 2 * idle timeout pass
        tokio::time::sleep(Duration::from_secs(2)).await;

        // trying to send a message should fail with an error
        match p1_conn
            .send((Bytes::new(), Bytes::new(), b"hello"[..].into()))
            .await
        {
            Err(SendError::ConnectionLost(ConnectionError::TimedOut)) => {}
            res => bail!("unexpected send result: {:?}", res),
        }

        // trying to receive should NOT return an error
        match p2_incoming.next().await {
            Err(_) => {}
            res => bail!("unexpected recv result: {:?}", res),
        }

        Ok(())
    }

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_endpoint_echo() -> Result<()> {
        let config = InternalConfig::try_from_config(Config::default())?;

        let mut peer1 = quinn::Endpoint::server(config.server.clone(), local_addr())?;
        peer1.set_default_client_config(config.client);

        let peer2 = quinn::Endpoint::server(config.server.clone(), local_addr())?;

        {
            let (p1_conn, _) = Connection::new(
                peer1.connect(peer2.local_addr()?, SERVER_NAME)?.await?,
                peer1.clone(),
            );

            // we need to accept the connection on p2, or the message won't be processed
            let _p2_handle = if let Some(connection) =
                timeout(OptionFuture::from(peer2.accept().await))
                    .await?
                    .and_then(|c| c.ok())
            {
                Connection::new(connection, peer2.clone())
            } else {
                bail!("did not receive incoming connection when one was expected");
            };

            let (mut send_stream, mut recv_stream) = p1_conn.open_bi().await?;
            send_stream.send_wire_msg(WireMsg::EndpointEchoReq).await?;

            let msg = timeout(recv_stream.read_wire_msg()).await??;
            if let WireMsg::EndpointEchoResp(addr) = msg {
                assert_eq!(addr, peer1.local_addr()?);
            } else {
                bail!(
                    "received unexpected message when EndpointEchoResp was expected: {:?}",
                    msg
                );
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

        let mut peer1 = quinn::Endpoint::server(config.server.clone(), local_addr())?;
        peer1.set_default_client_config(config.client.clone());

        let mut peer2 = quinn::Endpoint::server(config.server.clone(), local_addr())?;
        peer2.set_default_client_config(config.client);

        {
            let (p1_conn, _) = Connection::new(
                peer1.connect(peer2.local_addr()?, SERVER_NAME)?.await?,
                peer1.clone(),
            );

            // we need to accept the connection on p2, or the message won't be processed
            let _p2_handle = if let Some(connection) =
                timeout(OptionFuture::from(peer2.accept().await))
                    .await?
                    .and_then(|c| c.ok())
            {
                Connection::new(connection, peer2.clone())
            } else {
                bail!("did not receive incoming connection when one was expected");
            };

            let (mut send_stream, mut recv_stream) = p1_conn.open_bi().await?;
            send_stream
                .send_wire_msg(WireMsg::EndpointVerificationReq(peer1.local_addr()?))
                .await?;

            // we need to accept the connection on p1, or the message won't be processed
            let _p1_handle = if let Some(connection) =
                timeout(OptionFuture::from(peer1.accept().await))
                    .await?
                    .and_then(|c| c.ok())
            {
                Connection::new(connection, peer1.clone())
            } else {
                bail!("did not receive incoming connection when one was expected");
            };

            let msg = timeout(recv_stream.read_wire_msg()).await??;
            if let WireMsg::EndpointVerificationResp(true) = msg {
            } else {
                bail!(
                        "received unexpected message when EndpointVerificationResp(true) was expected: {:?}",
                        msg
                    );
            }

            // only one msg per bi-stream is supported, let's create a new bi-stream for this test
            let (mut send_stream, mut recv_stream) = p1_conn.open_bi().await?;
            send_stream
                .send_wire_msg(WireMsg::EndpointVerificationReq(local_addr()))
                .await?;

            let msg = timeout(recv_stream.read_wire_msg()).await??;
            if let WireMsg::EndpointVerificationResp(false) = msg {
            } else {
                bail!(
                        "received unexpected message when EndpointVerificationResp(false) was expected: {:?}",
                        msg
                    );
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
