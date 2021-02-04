// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::QuicP2p;

use super::{
    connection_pool::{ConnectionPool, ConnectionRemover},
    error::{Error, Result},
    wire_msg::WireMsg,
};
use bytes::Bytes;
use futures::stream::StreamExt;
use log::{error, trace};
use std::net::SocketAddr;
use tokio::sync::mpsc::UnboundedSender;
use tokio::{
    select,
    time::{timeout, Duration},
};

/// Connection instance to a node which can be used to send messages to it
#[derive(Clone)]
pub(crate) struct Connection {
    quic_conn: quinn::Connection,
    remover: ConnectionRemover,
}

impl Connection {
    pub(crate) fn new(quic_conn: quinn::Connection, remover: ConnectionRemover) -> Self {
        Self { quic_conn, remover }
    }

    pub async fn open_bi(&self) -> Result<(SendStream, RecvStream)> {
        let (send_stream, recv_stream) = self.handle_error(self.quic_conn.open_bi().await)?;
        Ok((SendStream::new(send_stream), RecvStream::new(recv_stream)))
    }

    /// Send message to peer using a uni-directional stream.
    pub async fn send_uni(&self, msg: Bytes) -> Result<()> {
        let mut send_stream = self.handle_error(self.quic_conn.open_uni().await)?;
        self.handle_error(send_msg(&mut send_stream, msg).await)?;

        // We try to make sure the stream is gracefully closed and the bytes get sent,
        // but if it was already closed (perhaps by the peer) then we
        // don't remove the connection from the pool.
        match send_stream.finish().await {
            Ok(()) | Err(quinn::WriteError::Stopped(_)) => Ok(()),
            Err(err) => {
                self.handle_error(Err(err))?;
                Ok(())
            }
        }
    }

    fn handle_error<T, E>(&self, result: Result<T, E>) -> Result<T, E> {
        if result.is_err() {
            self.remover.remove()
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

    /// Read next message from the stream
    pub async fn next(&mut self) -> Result<Bytes> {
        match read_bytes(&mut self.quinn_recv_stream).await {
            Ok(WireMsg::UserMsg(bytes)) => Ok(bytes),
            Ok(msg) => Err(Error::UnexpectedMessageType(msg)),
            Err(error) => Err(error),
        }
    }
}

impl std::fmt::Debug for RecvStream {
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

    /// Send a message using the stream created by the initiator
    pub async fn send_user_msg(&mut self, msg: Bytes) -> Result<()> {
        send_msg(&mut self.quinn_send_stream, msg).await
    }

    /// Send a wire message
    pub async fn send(&mut self, msg: WireMsg) -> Result<()> {
        msg.write_to_stream(&mut self.quinn_send_stream).await
    }

    /// Gracefully finish current stream
    pub async fn finish(mut self) -> Result<()> {
        self.quinn_send_stream.finish().await?;
        Ok(())
    }
}

impl std::fmt::Debug for SendStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "SendStream {{ .. }} ")
    }
}

// Helper to read the message's bytes from the provided stream
pub async fn read_bytes(recv: &mut quinn::RecvStream) -> Result<WireMsg> {
    WireMsg::read_from_stream(recv).await
}

// Helper to send bytes to peer using the provided stream.
pub async fn send_msg(mut send_stream: &mut quinn::SendStream, msg: Bytes) -> Result<()> {
    let wire_msg = WireMsg::UserMsg(msg);
    wire_msg.write_to_stream(&mut send_stream).await?;

    trace!("Message was sent to remote peer");

    Ok(())
}

pub(super) fn listen_for_incoming_connections(
    mut quinn_incoming: quinn::Incoming,
    connection_pool: ConnectionPool,
    message_tx: UnboundedSender<(SocketAddr, Bytes)>,
    connection_tx: UnboundedSender<SocketAddr>,
    disconnection_tx: UnboundedSender<SocketAddr>,
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
                        let pool_handle = connection_pool.insert(peer_address, connection);
                        let _ = connection_tx.send(peer_address);
                        listen_for_incoming_messages(
                            uni_streams,
                            bi_streams,
                            pool_handle,
                            message_tx.clone(),
                            disconnection_tx.clone(),
                        );
                    }
                    Err(err) => {
                        error!("An incoming connection failed because of an error: {}", err);
                    }
                },
                None => {
                    trace!("quinn::Incoming::next() returned None. There will be no more incoming connections");
                    break;
                }
            }
        }
        Ok::<_, Error>(())
    });
}

pub(super) fn listen_for_incoming_messages(
    mut uni_streams: quinn::IncomingUniStreams,
    mut bi_streams: quinn::IncomingBiStreams,
    remover: ConnectionRemover,
    message_tx: UnboundedSender<(SocketAddr, Bytes)>,
    disconnection_tx: UnboundedSender<SocketAddr>,
) {
    let src = *remover.remote_addr();
    let _ = tokio::spawn(async move {
        loop {
            let message: Option<Bytes> = select! {
                bytes = next_on_uni_streams(&mut uni_streams) => bytes,
                bytes = next_on_bi_streams(&mut bi_streams, src) => bytes,
            };
            if let Some(message) = message {
                // When the message in handled internally we return Bytes::new() to prevent
                // connection termination
                if !message.is_empty() {
                    let _ = message_tx.send((src, message));
                }
            } else {
                log::trace!("The connection has been terminated.");
                let _ = disconnection_tx.send(*remover.remote_addr());
                remover.remove();
                break;
            }
        }
        Ok::<_, Error>(())
    });
}

// Returns next message sent by peer in an unidirectional stream.
async fn next_on_uni_streams(uni_streams: &mut quinn::IncomingUniStreams) -> Option<Bytes> {
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
            Ok(WireMsg::UserMsg(bytes)) => Some(bytes),
            Ok(msg) => {
                error!("Unexpected message type: {:?}", msg);
                Some(Bytes::new())
            }
            Err(err) => {
                error!("{}", err);
                Some(Bytes::new())
            }
        },
    }
}

// Returns next message sent by peer in a bidirectional stream.
async fn next_on_bi_streams(
    bi_streams: &mut quinn::IncomingBiStreams,
    peer_addr: SocketAddr,
) -> Option<Bytes> {
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
        Some(Ok((mut send, mut recv))) => match read_bytes(&mut recv).await {
            Ok(WireMsg::UserMsg(bytes)) => Some(bytes),
            Ok(WireMsg::EndpointEchoReq) => {
                trace!("Received Echo Request");
                let message = WireMsg::EndpointEchoResp(peer_addr);
                message.write_to_stream(&mut send).await.ok()?;
                trace!("Responded to Echo request");
                Some(Bytes::new())
            }
            Ok(WireMsg::EndpointVerificationReq(address_sent)) => {
                trace!(
                    "Received Endpoint verification request {:?} from {:?}",
                    address_sent,
                    peer_addr
                );
                // Verify if the peer's endpoint is reachable via EchoServiceReq
                let qp2p = QuicP2p::with_config(Default::default(), &[], false).ok()?;
                let (temporary_endpoint, _, _, _) = qp2p.new_endpoint().await.ok()?;
                let (mut temp_send, mut temp_recv) = temporary_endpoint
                    .open_bidirectional_stream(&address_sent)
                    .await
                    .ok()?;
                let message = WireMsg::EndpointEchoReq;
                message
                    .write_to_stream(&mut temp_send.quinn_send_stream)
                    .await
                    .ok()?;
                let verified = matches!(
                    timeout(
                        Duration::from_secs(30),
                        WireMsg::read_from_stream(&mut temp_recv.quinn_recv_stream)
                    )
                    .await,
                    Ok(Ok(WireMsg::EndpointEchoResp(_)))
                );

                let message = WireMsg::EndpointVerficationResp(verified);
                message.write_to_stream(&mut send).await.ok()?;
                trace!("Responded to Endpoint verification request");
                Some(Bytes::new())
            }
            Ok(msg) => {
                error!("Unexpected message type: {:?}", msg);
                Some(Bytes::new())
            }
            Err(err) => {
                error!("{}", err);
                Some(Bytes::new())
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use anyhow::anyhow;

    use crate::api::QuicP2p;
    use crate::{config::Config, wire_msg::WireMsg, Error};
    use std::net::{IpAddr, Ipv4Addr};

    #[tokio::test]
    async fn echo_service() -> Result<(), Error> {
        let qp2p = QuicP2p::with_config(
            Some(Config {
                local_port: None,
                local_ip: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
                ..Config::default()
            }),
            Default::default(),
            false,
        )?;

        // Create Endpoint
        let (peer1, mut peer1_connections, _, _) = qp2p.new_endpoint().await?;
        let peer1_addr = peer1.socket_addr();

        let (peer2, _, _, _) = qp2p.new_endpoint().await?;
        let peer2_addr = peer2.socket_addr();

        peer2.connect_to(&peer1_addr).await?;

        if let Some(connecting_peer) = peer1_connections.next().await {
            assert_eq!(connecting_peer, peer2_addr);
        }

        let connection = peer1
            .get_connection(&peer2_addr)
            .ok_or(Error::MissingConnection)?;
        let (mut send_stream, mut recv_stream) = connection.open_bi().await?;
        let message = WireMsg::EndpointEchoReq;
        message
            .write_to_stream(&mut send_stream.quinn_send_stream)
            .await?;
        let message = WireMsg::read_from_stream(&mut recv_stream.quinn_recv_stream).await?;
        if let WireMsg::EndpointEchoResp(addr) = message {
            assert_eq!(addr, peer1_addr);
        } else {
            anyhow!("Unexpected response to EchoService request");
        }
        Ok(())
    }
}
