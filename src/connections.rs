// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use super::{
    api::Message,
    error::{Error, Result},
    wire_msg::WireMsg,
};
use bytes::Bytes;
use futures::{lock::Mutex, stream::StreamExt};
use log::{trace, warn};
use std::{net::SocketAddr, sync::Arc};
use tokio::select;

/// Connection instance to a node which can be used to send messages to it
pub struct Connection {
    quic_conn: quinn::Connection,
    max_msg_size: usize,
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.close();
    }
}

impl Connection {
    pub(crate) async fn new(quic_conn: quinn::Connection, max_msg_size: usize) -> Result<Self> {
        Ok(Self {
            quic_conn,
            max_msg_size,
        })
    }

    /// Remote address
    pub fn remote_address(&self) -> SocketAddr {
        self.quic_conn.remote_address()
    }

    /// Send message to peer and await for a reponse.
    pub async fn send(&mut self, msg: Bytes) -> Result<ConnectionStreams> {
        let (mut send_stream, recv_stream) = self.quic_conn.open_bi().await?;
        send_msg(&self.remote_address(), &mut send_stream, msg).await?;

        Ok(ConnectionStreams::new(
            self.remote_address(),
            send_stream,
            recv_stream,
            self.max_msg_size,
        ))
    }

    /// Send message to peer using a bi-directional stream without awaiting for a reponse.
    pub async fn send_only(&self, msg: Bytes) -> Result<()> {
        let (mut send_stream, _) = self.quic_conn.open_bi().await?;
        send_msg(&self.remote_address(), &mut send_stream, msg).await
    }

    /// Send message to peer using a uni-directional stream without awaiting for a reponse.
    pub async fn send_uni(&self, msg: Bytes) -> Result<()> {
        let mut send_stream = self.quic_conn.open_uni().await?;
        send_msg(&self.remote_address(), &mut send_stream, msg).await
    }

    /// Gracefully close connection immediatelly
    pub fn close(&self) {
        self.quic_conn.close(0u32.into(), b"");
    }
}

pub struct ConnectionStreams {
    peer_addr: SocketAddr,
    send_stream: quinn::SendStream,
    recv_stream: quinn::RecvStream,
    max_msg_size: usize,
}

impl ConnectionStreams {
    pub fn new(
        peer_addr: SocketAddr,
        send_stream: quinn::SendStream,
        recv_stream: quinn::RecvStream,
        max_msg_size: usize,
    ) -> Self {
        Self {
            peer_addr,
            send_stream,
            recv_stream,
            max_msg_size,
        }
    }

    pub async fn send(&mut self, msg: Bytes) -> Result<()> {
        send_msg(&self.peer_addr, &mut self.send_stream, msg).await
    }

    pub async fn next(self) -> Option<Bytes> {
        read_bytes(self.recv_stream, self.max_msg_size).await
    }
}

/// Stream of incoming QUIC connections
pub struct IncomingConnections {
    quinn_incoming: Arc<Mutex<quinn::Incoming>>,
    max_msg_size: usize,
}

impl IncomingConnections {
    pub(crate) fn new(
        quinn_incoming: Arc<Mutex<quinn::Incoming>>,
        max_msg_size: usize,
    ) -> Result<Self> {
        Ok(Self {
            quinn_incoming,
            max_msg_size,
        })
    }

    /// Returns next QUIC connection established by a peer
    pub async fn next(&mut self) -> Option<IncomingMessages> {
        match self.quinn_incoming.lock().await.next().await {
            Some(quinn_conn) => match quinn_conn.await {
                Ok(quinn::NewConnection {
                    connection,
                    uni_streams,
                    bi_streams,
                    ..
                }) => Some(IncomingMessages::new(
                    connection.remote_address(),
                    uni_streams,
                    bi_streams,
                    self.max_msg_size,
                )),
                Err(_err) => None,
            },
            None => None,
        }
    }
}

/// Stream of incoming QUIC messages
pub struct IncomingMessages {
    peer_addr: SocketAddr,
    uni_streams: quinn::IncomingUniStreams,
    bi_streams: quinn::IncomingBiStreams,
    max_msg_size: usize,
}

impl IncomingMessages {
    pub(crate) fn new(
        peer_addr: SocketAddr,
        uni_streams: quinn::IncomingUniStreams,
        bi_streams: quinn::IncomingBiStreams,
        max_msg_size: usize,
    ) -> Self {
        Self {
            peer_addr,
            uni_streams,
            bi_streams,
            max_msg_size,
        }
    }

    /// Returns the address of the peer who initiated the connection
    pub fn remote_addr(&self) -> SocketAddr {
        self.peer_addr
    }

    /// Returns next message sent by the peer on current QUIC connection,
    /// either received through a bi-directional or uni-directional stream.
    pub async fn next(&mut self) -> Option<Message> {
        // Each stream initiated by the remote peer constitutes a new message.
        // Read the next message available in any of the two type of streams.
        let src = self.peer_addr;
        select! {
            next_uni = Self::next_on_uni_streams(&mut self.uni_streams, self.max_msg_size) =>
                next_uni.map(|bytes| Message::UniStream {
                    bytes,
                    src
                }),
            next_bi = Self::next_on_bi_streams(&mut self.bi_streams, self.max_msg_size) =>
                next_bi.map(|(bytes, send)| Message::BiStream {
                    bytes,
                    src,
                    send: SendStream::new(send)
                }),
        }
    }

    // Returns next message sent by peer in an unidirectional stream.
    async fn next_on_uni_streams(
        uni_streams: &mut quinn::IncomingUniStreams,
        max_msg_size: usize,
    ) -> Option<Bytes> {
        match uni_streams.next().await {
            None => None,
            Some(Err(quinn::ConnectionError::ApplicationClosed { .. })) => {
                trace!("Connection terminated by peer.");
                None
            }
            Some(Err(err)) => {
                warn!("Failed to read incoming message on uni-stream: {}", err);
                None
            }
            Some(Ok(recv)) => read_bytes(recv, max_msg_size).await.map(|bytes| bytes),
        }
    }

    // Returns next message sent by peer in a bidirectional stream.
    async fn next_on_bi_streams(
        bi_streams: &mut quinn::IncomingBiStreams,
        max_msg_size: usize,
    ) -> Option<(Bytes, quinn::SendStream)> {
        match bi_streams.next().await {
            None => None,
            Some(Err(quinn::ConnectionError::ApplicationClosed { .. })) => {
                trace!("Connection terminated by peer.");
                None
            }
            Some(Err(err)) => {
                warn!("Failed to read incoming message on bi-stream: {}", err);
                None
            }
            Some(Ok((send, recv))) => read_bytes(recv, max_msg_size)
                .await
                .map(|bytes| (bytes, send)),
        }
    }
}

// Read the message's bytes which size is capped
async fn read_bytes(recv: quinn::RecvStream, max_msg_size: usize) -> Option<Bytes> {
    match recv.read_to_end(max_msg_size).await {
        Ok(wire_bytes) => {
            trace!("Got new message with {} bytes.", wire_bytes.len());
            match WireMsg::from_raw(wire_bytes) {
                Ok(WireMsg::UserMsg(msg_bytes)) => Some(Bytes::copy_from_slice(&msg_bytes)),
                Ok(WireMsg::EndpointEchoReq) | Ok(WireMsg::EndpointEchoResp(_)) => {
                    // TODO: handle the echo request/response message
                    warn!("UNIMPLEMENTED: echo message type not supported yet");
                    None
                }
                Err(err) => {
                    warn!("Failed to parse received message bytes: {}", err);
                    None
                }
            }
        }
        Err(err) => {
            warn!("Failed reading message's bytes: {}", err);
            None
        }
    }
}

// Private helper to send bytes to peer using the provided stream.
async fn send_msg(
    peer_addr: &SocketAddr,
    send_stream: &mut quinn::SendStream,
    msg: Bytes,
) -> Result<()> {
    // Let's generate the message bytes
    let wire_msg = WireMsg::UserMsg(msg);
    let (msg_bytes, msg_flag) = wire_msg.into();

    trace!(
        "Sending message to remote peer ({} bytes): {}",
        msg_bytes.len(),
        peer_addr
    );

    // Send message bytes over QUIC
    send_stream.write_all(&msg_bytes[..]).await?;

    // Then send message flag over QUIC
    send_stream.write_all(&[msg_flag]).await?;

    send_stream.finish().await?;

    trace!("Message was sent to remote peer: {}", peer_addr,);

    Ok(())
}

/// Stream of outgoing messages
pub struct SendStream {
    quinn_send_stream: quinn::SendStream,
}

impl SendStream {
    pub(crate) fn new(quinn_send_stream: quinn::SendStream) -> Self {
        Self { quinn_send_stream }
    }

    /// Send a message using the bi-direction stream created by the initiator
    pub async fn respond(&mut self, msg: &Bytes) -> Result<()> {
        self.quinn_send_stream.write_all(msg).await?;
        Ok(())
    }

    /// Gracefully finish current stream
    pub async fn finish(&mut self) -> Result<()> {
        self.quinn_send_stream.finish().await?;
        Ok(())
    }
}
