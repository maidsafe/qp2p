// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use super::wire_msg::WireMsg;
use crate::config::ConfigError;
use bytes::Bytes;
use std::{fmt, io};
use thiserror::Error;

/// Result used by `QuicP2p`.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Error types returned by the qp2p public API.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// quinn connection closed
    #[error("Connection was closed by {0}")]
    ConnectionClosed(Close),
    /// quinn connection error
    #[error("Connection lost due to error: {0}")]
    ConnectionError(#[source] ConnectionError),
    /// Error occurred when attempting to connect to any
    /// of the peers provided as a list of contacts.
    #[error("Network bootstrap failed")]
    BootstrapFailure,
    /// No peers/contacts found in the bootstrap nodes list.
    #[error("No nodes/peers found defined for bootstrapping")]
    EmptyBootstrapNodesList,
    /// I/O failure when attempting to access a local resource.
    #[error("I/O Error")]
    Io(#[from] io::Error),
    /// Failure encountered when establishing a connection with another peer.
    #[error("Establishing connection")]
    Connect(#[from] quinn::ConnectError),
    /// Failed to create a new endpoint.
    #[error("Creating endpoint")]
    Endpoint(#[from] quinn::EndpointError),
    /// Failed to set/get priority of stream.
    #[error("Unknown stream, cannot set/get priority.")]
    UnknownStream,
    /// The contacts list was found empty when attempting
    /// to contact peers for the echo service.
    #[error("No peers found in the contacts list to send the echo request to")]
    NoEchoServerEndpointDefined,
    /// Timeout occurred when awaiting for a response from
    /// any of the peers contacted for the echo service.
    #[error("No response received from echo services")]
    NoEchoServiceResponse,
    /// Failure occurred when sending an echo request.
    #[error("{0}")]
    EchoServiceFailure(String),
    /// Serialisation error, which can happen on different type of data.
    #[error("Serialisation error")]
    Serialisation(#[from] bincode::Error),
    /// Cannot assign port to endpoint
    #[error("Cannot assign port to endpoint {0}")]
    CannotAssignPort(u16),
    /// The message type flag decoded in an incoming stream is invalid/unsupported.
    #[error("Invalid message type flag found in message's header: {0}")]
    InvalidMsgFlag(u8),
    /// Error occurred when trying to write on a currently opened message stream.
    #[error("Stream write error")]
    StreamWrite(#[from] quinn::WriteError),
    /// The expected amount of message bytes couldn't be read from the stream.
    #[error("Failed to read expected number of message bytes: {0}")]
    StreamRead(#[from] quinn::ReadExactError),
    /// Failure when trying to map a new port using IGD for automatic port forwarding.
    #[error("Could not use IGD for automatic port forwarding")]
    IgdAddPort(#[from] igd::AddAnyPortError),
    /// Failure when trying to renew leasing of a port mapped using IGD.
    #[error("Could not renew port mapping using IGD")]
    IgdRenewPort(#[from] igd::AddPortError),
    /// IGD gateway deice was not found.
    #[error("Could not find the gateway device for IGD")]
    IgdSearch(#[from] igd::SearchError),
    /// IGD is not supported on IPv6
    #[error("IGD is not supported on IPv6")]
    IgdNotSupported,
    /// Response message received contains an empty payload.
    #[error("Empty response message received from peer")]
    EmptyResponse,
    /// The type of message received is not the expected one.
    #[error("Type of the message received was not the expected one: {0}")]
    UnexpectedMessageType(WireMsg),
    /// The message exceeds the maximum message length allowed.
    #[error("Maximum data length exceeded, length: {0}")]
    MaxLengthExceeded(usize),
    /// Incorrect Public Address provided
    #[error("Incorrect Public Address provided")]
    IncorrectPublicAddress,
    /// Missing connection
    #[error("No connection to the dest peer")]
    MissingConnection,
    /// Couldn't resolve Public IP address
    #[error("Unresolved Public IP address")]
    UnresolvedPublicIp,
}

impl From<quinn::ConnectionError> for Error {
    fn from(error: quinn::ConnectionError) -> Self {
        match error.into() {
            ConnectionCloseOrError::Close(close) => Self::ConnectionClosed(close),
            ConnectionCloseOrError::Error(error) => Self::ConnectionError(error),
        }
    }
}

impl From<ConnectFailed> for Error {
    fn from(error: ConnectFailed) -> Self {
        match error {
            ConnectFailed::Connect(error) => Self::Connect(error),
            ConnectFailed::Connection(error) => Self::ConnectionError(error),
            ConnectFailed::Close(close) => Self::ConnectionClosed(close),
        }
    }
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
}

impl From<quinn::EndpointError> for ClientEndpointError {
    fn from(error: quinn::EndpointError) -> Self {
        match error {
            quinn::EndpointError::Socket(error) => Self::Socket(error),
        }
    }
}

/// The reason a connection was closed.
#[derive(Clone, Debug)]
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
            Self::Local => return write!(f, "us"),
            Self::Application { error_code, reason } => {
                ("application", error_code, String::from_utf8_lossy(reason))
            }
            Self::Transport { error_code, reason } => {
                ("transport", error_code, String::from_utf8_lossy(reason))
            }
        };
        write!(
            f,
            "{} (error code: {}, reason: {})",
            closed_by, error_code, reason
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

/// Errors that can cause connection loss.
// This is a copy of `quinn::ConnectionError` without the `*Closed` variants, since we want to
// separate them in our interface.
#[derive(Clone, Debug, Error)]
pub enum ConnectionError {
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
}

/// An opaque error code indicating a transport failure.
///
/// This can be turned to a string via its `Debug` and `Display` impls, but is otherwise opaque.
#[derive(Clone)]
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

/// Errors preventing a `Connection` from being created.
///
/// The `quinn` API means that, before we can get a handle to a `Connection`, we have to handle both
/// `ConnectError` (indicating a misconfiguration, such that quinn didn't even try to open a
/// connection) and `ConnectionError` (indicating an I/O or protocol error when trying to open a
/// connection).
///
/// In our public API, we also want to separate quinn's `ConnectionError` into our own
/// `ConnectionError`/`Close`. This enum is intended to facilitate this. Note that it does not
/// `impl Error` and is not meant to be public.
#[derive(Clone, Debug)]
pub(crate) enum ConnectFailed {
    Connect(quinn::ConnectError),
    Connection(ConnectionError),
    Close(Close),
}

impl From<quinn::ConnectError> for ConnectFailed {
    fn from(error: quinn::ConnectError) -> Self {
        Self::Connect(error)
    }
}

impl From<quinn::ConnectionError> for ConnectFailed {
    fn from(error: quinn::ConnectionError) -> Self {
        match error.into() {
            ConnectionCloseOrError::Close(close) => Self::Close(close),
            ConnectionCloseOrError::Error(error) => Self::Connection(error),
        }
    }
}

/// Errors that can interrupt a connection.
///
/// We want to separate "close" and "error" in our API, to enable more robust error handling. This
/// is a helper enum for separating `quinn::ConnectionError` into those parts.
enum ConnectionCloseOrError {
    Close(Close),
    Error(ConnectionError),
}

impl From<quinn::ConnectionError> for ConnectionCloseOrError {
    fn from(error: quinn::ConnectionError) -> Self {
        match error {
            quinn::ConnectionError::LocallyClosed => Self::Close(Close::Local),
            quinn::ConnectionError::ApplicationClosed(close) => Self::Close(Close::Application {
                error_code: close.error_code.into_inner(),
                reason: close.reason,
            }),
            quinn::ConnectionError::ConnectionClosed(close) => Self::Close(Close::Transport {
                error_code: TransportErrorCode(close.error_code),
                reason: close.reason,
            }),
            quinn::ConnectionError::VersionMismatch => {
                Self::Error(ConnectionError::VersionMismatch)
            }
            quinn::ConnectionError::TransportError(error) => {
                Self::Error(ConnectionError::TransportError(error))
            }
            quinn::ConnectionError::Reset => Self::Error(ConnectionError::Reset),
            quinn::ConnectionError::TimedOut => Self::Error(ConnectionError::Reset),
        }
    }
}
