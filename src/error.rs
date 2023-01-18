// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::config::ConfigError;
use bytes::Bytes;
use std::{fmt, io, net::SocketAddr};
use thiserror::Error;

/// Errors returned from [`Endpoint::new`](crate::Endpoint::new).
#[derive(Debug, Error)]
pub enum EndpointError {
    /// There was a problem with the provided configuration.
    #[error("There was a problem with the provided configuration")]
    Config(#[from] ConfigError),

    /// Failed to bind UDP socket.
    #[error("Failed to bind UDP socket")]
    Socket(#[source] io::Error),

    /// Io error.
    #[error(transparent)]
    IoError(#[from] io::Error),
}

/// Errors returned by [`Endpoint::new_client`](crate::Endpoint::new_client).
#[derive(Debug, Error)]
pub enum ClientEndpointError {
    /// There was a problem with the provided configuration.
    #[error("There was a problem with the provided configuration")]
    Config(#[from] ConfigError),

    /// Failed to bind UDP socket.
    #[error("Failed to bind UDP socket")]
    Socket(#[source] io::Error),

    /// Io error.
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Errors that can cause connection loss.
// This is a copy of `quinn::ConnectionError` without the `*Closed` variants, since we want to
// separate them in our interface.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ConnectionError {
    /// The endpoint has been stopped.
    #[error("The endpoint has been stopped")]
    Stopped,

    /// The number of active connections on the local endpoint is at the limit.
    ///
    /// This limit is imposed by the underlying connection ID generator, which is not currently
    /// configurable.
    // NOTE: We could make this configurable by exposing a way to set
    // quinn_proto::RandomConnectionIdGenerator cid_len
    #[error("The number of active connections on the local endpoint is at the limit")]
    TooManyConnections,

    /// Invalid remote address.
    ///
    /// Examples include attempting to connect to port `0` or using an inappropriate address family.
    #[error("Invalid remote address: {0}")]
    InvalidAddress(SocketAddr),

    /// Internal configuration error.
    ///
    /// This should not occur (if it does, there's a bug!), but it covers possible misconfigurations
    /// of the underlying transport library.
    #[error("BUG: internal configuration error")]
    InternalConfigError(#[source] InternalConfigError),

    /// The peer doesn't implement the supported version.
    #[error("{}", quinn::ConnectionError::VersionMismatch)]
    VersionMismatch,

    /// The peer violated the QUIC specification as understood by this implementation.
    #[error("{0}")]
    TransportError(#[source] quinn_proto::TransportError),

    /// The peer is unable to continue processing this connection, usually due to having restarted.
    #[error("{}", quinn::ConnectionError::Reset)]
    Reset,

    /// Communication with the peer has lapsed for longer than the negotiated idle timeout.
    #[error("{}", quinn::ConnectionError::TimedOut)]
    TimedOut,

    /// The connection was closed.
    #[error("The connection was closed, {0}")]
    Closed(Close),
}

impl From<quinn::ConnectError> for ConnectionError {
    fn from(error: quinn::ConnectError) -> Self {
        match error {
            quinn::ConnectError::EndpointStopping => Self::Stopped,
            quinn::ConnectError::TooManyConnections => Self::TooManyConnections,
            quinn::ConnectError::InvalidRemoteAddress(addr) => Self::InvalidAddress(addr),
            quinn::ConnectError::InvalidDnsName(_)
            | quinn::ConnectError::NoDefaultClientConfig
            | quinn::ConnectError::UnsupportedVersion => {
                // We currently use a hard-coded domain name, so if it's invalid we have a library
                // breaking bug. We want to avoid panics though, so we propagate this as an opaque
                // error.
                Self::InternalConfigError(InternalConfigError(error))
            }
        }
    }
}

impl From<quinn::ConnectionError> for ConnectionError {
    fn from(error: quinn::ConnectionError) -> Self {
        match error {
            quinn::ConnectionError::LocallyClosed => Self::Closed(Close::Local),
            quinn::ConnectionError::ApplicationClosed(close) => Self::Closed(Close::Application {
                error_code: close.error_code.into_inner(),
                reason: close.reason,
            }),
            quinn::ConnectionError::ConnectionClosed(close) => Self::Closed(Close::Transport {
                error_code: TransportErrorCode(close.error_code),
                reason: close.reason,
            }),
            quinn::ConnectionError::VersionMismatch => Self::VersionMismatch,
            quinn::ConnectionError::TransportError(error) => Self::TransportError(error),
            quinn::ConnectionError::Reset => Self::Reset,
            quinn::ConnectionError::TimedOut => Self::TimedOut,
        }
    }
}

/// An internal configuration error encountered by [`Endpoint`](crate::Endpoint) connect methods.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error(transparent)]
pub struct InternalConfigError(quinn::ConnectError);

/// The reason a connection was closed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Close {
    /// This application closed the connection.
    Local,

    /// The remote application closed the connection.
    Application {
        /// The error code supplied by the application.
        error_code: u64,

        /// The reason supplied by the application.
        reason: Bytes,
    },

    /// The transport layer closed the connection.
    ///
    /// This would indicate an abrupt disconnection or a protocol violation.
    Transport {
        /// The error code supplied by the QUIC library.
        error_code: TransportErrorCode,

        /// The reason supplied by the QUIC library.
        reason: Bytes,
    },
}

impl fmt::Display for Close {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let (closed_by, error_code, reason): (_, &dyn fmt::Display, _) = match self {
            Self::Local => return write!(f, "we closed the connection"),
            Self::Application { error_code, reason } => ("remote application", error_code, reason),
            Self::Transport { error_code, reason } => ("transport layer", error_code, reason),
        };
        write!(
            f,
            "{} closed the connection (error code: {}, reason: {})",
            closed_by,
            error_code,
            String::from_utf8_lossy(reason)
        )
    }
}

impl From<quinn::ApplicationClose> for Close {
    fn from(close: quinn::ApplicationClose) -> Self {
        Self::Application {
            error_code: close.error_code.into_inner(),
            reason: close.reason,
        }
    }
}

impl From<quinn::ConnectionClose> for Close {
    fn from(close: quinn::ConnectionClose) -> Self {
        Self::Transport {
            error_code: TransportErrorCode(close.error_code),
            reason: close.reason,
        }
    }
}

/// An opaque error code indicating a transport failure.
///
/// This can be turned to a string via its `Debug` and `Display` impls, but is otherwise opaque.
#[derive(Clone, PartialEq, Eq)]
pub struct TransportErrorCode(quinn_proto::TransportErrorCode);

impl fmt::Debug for TransportErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl fmt::Display for TransportErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Errors that can occur when performing an RPC operation.
///
/// QuicP2P uses a number of RPC operations when establishing endpoints, in order to verify public
/// addresses and check for reachability. This error type covers the ways in which these operations
/// can fail, which is a combination of [`SendError`], and [`RecvError`].
#[derive(Debug, Error)]
pub enum RpcError {
    /// We did not receive a response within the expected time.
    #[error("We did not receive a response within the expected time")]
    TimedOut,

    /// Failed to send the request.
    #[error("Failed to send the request")]
    Send(#[from] SendError),

    /// Failed to receive the response.
    #[error("Failed to receive the response")]
    Recv(#[from] RecvError),
}

// Treating `ConnectionError`s as happening on send works because we would only encounter them
// directly (e.g. not part of `SendError` or `RecvError`) when establishing outgoing connections
// before sending.
impl From<ConnectionError> for RpcError {
    fn from(error: ConnectionError) -> Self {
        Self::Send(error.into())
    }
}

impl From<quinn::ConnectionError> for RpcError {
    fn from(error: quinn::ConnectionError) -> Self {
        Self::Send(error.into())
    }
}

impl From<tokio::time::error::Elapsed> for RpcError {
    fn from(_: tokio::time::error::Elapsed) -> Self {
        Self::TimedOut
    }
}

/// Errors that can occur when sending messages.
#[derive(Debug, Error)]
pub enum SendError {
    /// The serialised message is too long
    #[error("The serialized message is too long ({0} bytes, max: 4 GiB)")]
    MessageTooLong(usize),

    /// Failed to serialize message.
    ///
    /// This likely indicates a bug in the library, since serializing to bytes should be infallible.
    /// Limitations in the serde API mean we cannot verify this statically, and we don't want to
    /// introduce potential panics.
    #[error("Failed to serialize message")]
    Serialization(#[from] bincode::Error),

    /// Connection was lost when trying to send a message.
    #[error("Connection was lost when trying to send a message")]
    ConnectionLost(#[from] ConnectionError),

    /// Stream was lost when trying to send a message.
    #[error("Stream was lost when trying to send a message")]
    StreamLost(#[source] StreamError),
}

impl From<quinn::ConnectionError> for SendError {
    fn from(error: quinn::ConnectionError) -> Self {
        Self::ConnectionLost(error.into())
    }
}

impl From<quinn::WriteError> for SendError {
    fn from(error: quinn::WriteError) -> Self {
        match error {
            quinn::WriteError::Stopped(code) => Self::StreamLost(StreamError::Stopped(code.into())),
            quinn::WriteError::ConnectionLost(error) => Self::ConnectionLost(error.into()),
            quinn::WriteError::UnknownStream => Self::StreamLost(StreamError::Gone),
            quinn::WriteError::ZeroRttRejected => Self::StreamLost(StreamError::Unsupported(
                UnsupportedStreamOperation(error.into()),
            )),
        }
    }
}

/// Errors that can occur when receiving messages.
#[derive(Debug, Error)]
pub enum RecvError {
    /// Message received has an empty payload
    #[error("Message received from peer has an empty payload")]
    EmptyMsgPayload,

    /// Not enough bytes were received to be a valid message
    #[error("Received too few bytes for message")]
    NotEnoughBytes,

    /// Failed to deserialize message.
    #[error("Failed to deserialize message")]
    Serialization(#[from] bincode::Error),

    /// Message type flag found in message header is invalid
    #[error("Invalid message type flag found in message header: {0}")]
    InvalidMsgTypeFlag(u8),

    /// Type of message received is unexpected
    #[error("Type of message received is unexpected: {0}")]
    UnexpectedMsgReceived(String),

    /// Connection was lost when trying to receive a message.
    #[error("Connection was lost when trying to receive a message")]
    ConnectionLost(#[from] ConnectionError),

    /// Stream was lost when trying to receive a message.
    #[error("Stream was lost when trying to receive a message")]
    StreamLost(#[source] StreamError),
}

impl From<quinn::ConnectionError> for RecvError {
    fn from(error: quinn::ConnectionError) -> Self {
        Self::ConnectionLost(error.into())
    }
}

impl From<quinn::ReadError> for RecvError {
    fn from(error: quinn::ReadError) -> Self {
        use quinn::ReadError;

        match error {
            ReadError::Reset(code) => Self::StreamLost(StreamError::Stopped(code.into())),
            ReadError::ConnectionLost(error) => Self::ConnectionLost(error.into()),
            ReadError::UnknownStream => Self::StreamLost(StreamError::Gone),
            ReadError::IllegalOrderedRead | ReadError::ZeroRttRejected => Self::StreamLost(
                StreamError::Unsupported(UnsupportedStreamOperation(error.into())),
            ),
        }
    }
}

impl From<quinn::ReadExactError> for RecvError {
    fn from(error: quinn::ReadExactError) -> Self {
        match error {
            quinn::ReadExactError::FinishedEarly => Self::NotEnoughBytes,
            quinn::ReadExactError::ReadError(error) => error.into(),
        }
    }
}

/// Errors that can occur when interacting with streams.
#[derive(Debug, Error)]
pub enum StreamError {
    /// The peer abandoned the stream.
    #[error("The peer abandoned the stream (error code: {0})")]
    Stopped(u64),

    /// The stream was already stopped, finished, or reset.
    #[error("The stream was already stopped, finished, or reset")]
    Gone,

    /// An error was caused by an unsupported operation.
    ///
    /// Additional stream errors can arise from the use of 0-RTT connections or unordered reads,
    /// neither of which are supported by the library.
    #[error("An error was caused by an unsupported operation")]
    Unsupported(#[source] UnsupportedStreamOperation),
}

/// An error caused by an unsupported operation.
#[derive(Debug, Error)]
#[error(transparent)]
pub struct UnsupportedStreamOperation(Box<dyn std::error::Error + Send + Sync>);
