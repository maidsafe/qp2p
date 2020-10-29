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
use log::{error, trace};
use std::{net::SocketAddr, sync::Arc};
use tokio::select;

/// Connection instance to a node which can be used to send messages to it
pub struct Connection {
    quic_conn: quinn::Connection,
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.close();
    }
}

impl Connection {
    pub(crate) async fn new(quic_conn: quinn::Connection) -> Result<Self> {
        Ok(Self { quic_conn })
    }

    /// Returns the address of the connected peer.
    ///
    /// # Example
    ///
    /// ```
    /// use qp2p::{QuicP2p, Config, Error};
    /// use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Error> {
    ///
    ///     let mut config = Config::default();
    ///     config.ip = Some(IpAddr::V4(Ipv4Addr::LOCALHOST));
    ///     let mut quic_p2p = QuicP2p::with_config(Some(config.clone()), Default::default(), true)?;
    ///     let peer_1 = quic_p2p.new_endpoint()?;
    ///     let peer1_addr = peer_1.our_addr()?;
    ///
    ///     let (peer_2, connection) = quic_p2p.connect_to(&peer1_addr).await?;
    ///     assert_eq!(connection.remote_address(), peer1_addr);
    ///     Ok(())
    /// }
    /// ```
    pub fn remote_address(&self) -> SocketAddr {
        self.quic_conn.remote_address()
    }

    /// Get connection streams for reading/writing
    ///
    /// # Example
    ///
    /// ```
    /// use qp2p::{QuicP2p, Config, Error};
    /// use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Error> {
    ///
    ///     let mut config = Config::default();
    ///     config.ip = Some(IpAddr::V4(Ipv4Addr::LOCALHOST));
    ///     let mut quic_p2p = QuicP2p::with_config(Some(config.clone()), Default::default(), true)?;
    ///     let peer_1 = quic_p2p.new_endpoint()?;
    ///     let peer1_addr = peer_1.our_addr()?;
    ///
    ///     let (peer_2, connection) = quic_p2p.connect_to(&peer1_addr).await?;
    ///     let (send_stream, recv_stream) = connection.open_bi_stream().await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn open_bi_stream(&self) -> Result<(SendStream, RecvStream)> {
        let (send_stream, recv_stream) = self.quic_conn.open_bi().await?;
        Ok((SendStream::new(send_stream), RecvStream::new(recv_stream)))
    }

    /// Send message to the connected peer via a bi-directional stream.
    /// This returns the streams to send additional messages / read responses sent using the same stream.
    pub async fn send(&self, msg: Bytes) -> Result<(SendStream, RecvStream)> {
        let (mut send_stream, recv_stream) = self.open_bi_stream().await?;
        send_stream.send(msg).await?;
        Ok((send_stream, recv_stream))
    }

    /// Send message to peer using a uni-directional stream.
    pub async fn send_uni(&self, msg: Bytes) -> Result<()> {
        let mut send_stream = self.quic_conn.open_uni().await?;
        send_msg(&mut send_stream, msg).await?;
        send_stream.finish().await.map_err(Error::from)
    }

    /// Gracefully close connection immediatelly
    pub fn close(&self) {
        self.quic_conn.close(0u32.into(), b"");
    }
}

/// Stream of incoming QUIC connections
pub struct IncomingConnections {
    quinn_incoming: Arc<Mutex<quinn::Incoming>>,
}

impl IncomingConnections {
    pub(crate) fn new(quinn_incoming: Arc<Mutex<quinn::Incoming>>) -> Result<Self> {
        Ok(Self { quinn_incoming })
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
}

impl IncomingMessages {
    pub(crate) fn new(
        peer_addr: SocketAddr,
        uni_streams: quinn::IncomingUniStreams,
        bi_streams: quinn::IncomingBiStreams,
    ) -> Self {
        Self {
            peer_addr,
            uni_streams,
            bi_streams,
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
            next_uni = Self::next_on_uni_streams(&mut self.uni_streams) =>
                next_uni.map(|(bytes, recv)| Message::UniStream {
                    bytes,
                    src,
                    recv: RecvStream::new(recv)
                }),
            next_bi = Self::next_on_bi_streams(&mut self.bi_streams) =>
                next_bi.map(|(bytes, send, recv)| Message::BiStream {
                    bytes,
                    src,
                    send: SendStream::new(send),
                    recv: RecvStream::new(recv)
                }),
        }
    }

    // Returns next message sent by peer in an unidirectional stream.
    async fn next_on_uni_streams(
        uni_streams: &mut quinn::IncomingUniStreams,
    ) -> Option<(Bytes, quinn::RecvStream)> {
        match uni_streams.next().await {
            None => None,
            Some(Err(quinn::ConnectionError::ApplicationClosed { .. })) => {
                trace!("Connection terminated by peer.");
                None
            }
            Some(Err(err)) => {
                error!("Failed to read incoming message on uni-stream: {}", err);
                None
            }
            Some(Ok(mut recv)) => match read_bytes(&mut recv).await {
                Ok(bytes) => Some((bytes, recv)),
                Err(err) => {
                    error!("{}", err);
                    None
                }
            },
        }
    }

    // Returns next message sent by peer in a bidirectional stream.
    async fn next_on_bi_streams(
        bi_streams: &mut quinn::IncomingBiStreams,
    ) -> Option<(Bytes, quinn::SendStream, quinn::RecvStream)> {
        match bi_streams.next().await {
            None => None,
            Some(Err(quinn::ConnectionError::ApplicationClosed { .. })) => {
                trace!("Connection terminated by peer.");
                None
            }
            Some(Err(err)) => {
                error!("Failed to read incoming message on bi-stream: {}", err);
                None
            }
            Some(Ok((send, mut recv))) => match read_bytes(&mut recv).await {
                Ok(bytes) => Some((bytes, send, recv)),
                Err(err) => {
                    error!("{}", err);
                    None
                }
            },
        }
    }
}

/// Stream to receive multiple messages
pub struct RecvStream {
    quinn_recv_stream: quinn::RecvStream,
}

impl RecvStream {
    pub(crate) fn new(quinn_recv_stream: quinn::RecvStream) -> Self {
        Self { quinn_recv_stream }
    }

    /// Read next message from the stream
    pub async fn next(&mut self) -> Result<Bytes> {
        read_bytes(&mut self.quinn_recv_stream).await
    }
}

/// Stream of outgoing messages
pub struct SendStream {
    quinn_send_stream: quinn::SendStream,
}

impl SendStream {
    pub(crate) fn new(quinn_send_stream: quinn::SendStream) -> Self {
        Self { quinn_send_stream }
    }

    /// Send a message using the bi-directional stream created by the initiator
    pub async fn send(&mut self, msg: Bytes) -> Result<()> {
        send_msg(&mut self.quinn_send_stream, msg).await
    }

    /// Gracefully finish current stream
    pub async fn finish(mut self) -> Result<()> {
        self.quinn_send_stream.finish().await.map_err(Error::from)
    }
}

// Helper to read the message's bytes from the provided stream
async fn read_bytes(recv: &mut quinn::RecvStream) -> Result<Bytes> {
    match WireMsg::read_from_stream(recv).await? {
        WireMsg::UserMsg(msg_bytes) => Ok(msg_bytes),
        WireMsg::EndpointEchoReq | WireMsg::EndpointEchoResp(_) => {
            // TODO: handle the echo request/response message
            unimplemented!("echo message type not supported yet");
        }
    }
}

// Helper to send bytes to peer using the provided stream.
async fn send_msg(mut send_stream: &mut quinn::SendStream, msg: Bytes) -> Result<()> {
    let wire_msg = WireMsg::UserMsg(msg);
    wire_msg.write_to_stream(&mut send_stream).await?;

    trace!("Message was sent to remote peer");

    Ok(())
}
