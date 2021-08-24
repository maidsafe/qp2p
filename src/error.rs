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
use std::{fmt, io, net::SocketAddr};
use thiserror::Error;

/// Result used by `QuicP2p`.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Error types returned by the qp2p public API.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// quinn connection error
    #[error("Connection lost due to error: {0}")]
    ConnectionError(#[from] ConnectionError),
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
    /// Failed to create a new endpoint.
    #[error("Creating endpoint")]
    Endpoint(#[from] quinn::EndpointError),
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
    #[error(transparent)]
    UnexpectedMessageType(#[from] UnexpectedMessageType),
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
        Self::ConnectionError(error.into())
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

/// Errors that can cause connection loss.
// This is a copy of `quinn::ConnectionError` without the `*Closed` variants, since we want to
// separate them in our interface.
#[derive(Clone, Debug, Error, PartialEq)]
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
    #[error("The connection was closed by {0}")]
    Closed(Close),
}

impl From<quinn::ConnectError> for ConnectionError {
    fn from(error: quinn::ConnectError) -> Self {
        match error {
            quinn::ConnectError::EndpointStopping => Self::Stopped,
            quinn::ConnectError::TooManyConnections => Self::TooManyConnections,
            quinn::ConnectError::InvalidRemoteAddress(addr) => Self::InvalidAddress(addr),
            quinn::ConnectError::InvalidDnsName(_) => {
                // We currently use a hard-coded domain name, so if it's invalid we have a library
                // breaking bug. We want to avoid panics though, so we propagate this as an opaque
                // error.
                Self::InternalConfigError(InternalConfigError(error))
            }
            quinn::ConnectError::Config(_) => {
                // This is logically impossible, but has not been removed from the quinn API
                // (PR: https://github.com/quinn-rs/quinn/pull/1181). As above, we don't want to
                // allow any panics, so we propagate it as an opaque internal error.
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
#[derive(Clone, Debug, Error, PartialEq)]
#[error(transparent)]
pub struct InternalConfigError(quinn::ConnectError);

/// The reason a connection was closed.
#[derive(Clone, Debug, PartialEq)]
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

/// An opaque error code indicating a transport failure.
///
/// This can be turned to a string via its `Debug` and `Display` impls, but is otherwise opaque.
#[derive(Clone, PartialEq)]
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

/// The type of message received is not the expected one.
#[derive(Debug, Error)]
#[error("The of the message received was not the expected one: {0}")]
pub struct UnexpectedMessageType(WireMsg);

impl From<WireMsg> for UnexpectedMessageType {
    fn from(msg: WireMsg) -> Self {
        UnexpectedMessageType(msg)
    }
}
