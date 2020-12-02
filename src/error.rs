// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use err_derive::Error;
use std::net::SocketAddr;
use std::{io, sync::mpsc};

/// Result used by `QuicP2p`.
pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Error)]
#[allow(missing_docs)]
pub enum Error {
    #[error(display = "Network bootstrap failed")]
    BootstrapFailure,
    #[error(display = "I/O Error")]
    Io(#[source] io::Error),
    #[error(display = "quinn read")]
    Read(#[source] quinn::ReadError),
    #[error(display = "Bi-directional stream attempted by peer {}", 0)]
    BiDirectionalStreamAttempted(SocketAddr),
    #[error(display = "Establishing connection")]
    Connect(#[source] quinn::ConnectError),
    #[error(display = "Connection lost")]
    Connection(#[source] quinn::ConnectionError),
    #[error(display = "Creating endpoint")]
    Endpoint(#[source] quinn::EndpointError),
    #[error(display = "Cannot parse certificate ")]
    CertificateParseError,
    #[error(display = "Already connected {}", 0)]
    DuplicateConnectionToPeer(SocketAddr),
    #[error(display = "Could not find endpoint server")]
    NoEndpointEchoServerFound,
    #[error(display = "No response from echo service")]
    NoEchoServiceResponse,
    #[error(display = "Oneshot receiver")]
    OneShotRx(#[source] tokio::sync::oneshot::error::RecvError),
    #[error(display = "TLS Error ")]
    TLS(#[source] rustls::TLSError),
    #[error(display = "Bincode serialisation")]
    Bincode(#[source] bincode::Error),
    #[error(display = "Base64 decode ")]
    Base64(#[source] base64::DecodeError),
    #[error(display = "Configuration {}", 0)]
    Configuration(String),
    #[error(display = "Operation not allowed")]
    OperationNotAllowed,
    #[error(display = "Connection cancelled")]
    ConnectionCancelled,
    #[error(display = "Channel receive error")]
    ChannelRecv(#[source] mpsc::RecvError),
    #[error(display = "Could not add certificate to PKI")]
    WebPki,
    #[error(display = "Invalid wire message.")]
    InvalidWireMsgFlag,
    #[error(display = "Stream write error")]
    WriteError(#[source] quinn::WriteError),
    #[error(display = "Read to end error: {}", 0)]
    ReadToEndError(#[source] quinn::ReadToEndError),
    #[error(display = "Read exact error: {}", 0)]
    ReadExactError(#[source] quinn::ReadExactError),
    #[error(display = "Could not add certificate")]
    AddCertificateError(#[source] quinn::ParseError),
    #[error(display = "Could not use IGD for automatic port forwarding")]
    IgdAddPort(#[source] igd::AddAnyPortError),
    #[error(display = "Could not renew port mapping using IGD")]
    IgdRenewPort(#[source] igd::AddPortError),
    #[error(display = "Could not find the gateway device for IGD")]
    IgdSearch(#[source] igd::SearchError),
    #[error(display = "JoinError")]
    JoinError(#[source] tokio::task::JoinError),
    #[error(display = "RcGen error: {}", 0)]
    RcGen(#[source] rcgen::RcgenError),
    #[error(display = "IGD is not supported")]
    IgdNotSupported,
    #[error(display = "Empty response message received from peer")]
    EmptyResponse,
    #[error(display = "Type of the message received was not the expected one")]
    UnexpectedMessageType,
    #[error(display = "Maximum data length exceeded")]
    MaxLengthExceeded,
    #[error(display = "Mo incoming connection")]
    NoIncomingConnection,
    #[error(display = "No incoming message")]
    NoIncomingMessage,
    #[error(display = "Unexpected: {}", 0)]
    Unexpected(String),
}
