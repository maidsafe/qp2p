// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use err_derive::Error;

use std::io;
use std::net::SocketAddr;
use std::sync::mpsc;

#[derive(Debug, Error)]
#[allow(missing_docs)]
pub enum QuicP2pError {
    #[error(display = "I/O Error")]
    Io(#[source] io::Error),
    #[error(display = "quinn read")]
    Read(#[source] quinn::ReadError),
    #[error(display = "Bi-directional stream attempted by peer {}", peer_addr)]
    BiDirectionalStreamAttempted { peer_addr: SocketAddr },
    #[error(display = "Establishing connection")]
    Connect(#[source] quinn::ConnectError),
    #[error(display = "Connection lost")]
    Connection(#[source] quinn::ConnectionError),
    #[error(display = "Creating endpoint")]
    Endpoint(#[source] quinn::EndpointError),
    #[error(display = "Cannot parse certificate ")]
    CertificateParseError,
    #[error(display = "Already connected {}", peer_addr)]
    DuplicateConnectionToPeer { peer_addr: SocketAddr },
    #[error(display = "Could not find enpoint server")]
    NoEndpointEchoServerFound,
    #[error(display = "Oneshot receiver ")]
    OneShotRx(#[source] tokio::sync::oneshot::error::RecvError),
    #[error(display = "TLS Error ")]
    TLS(#[source] rustls::TLSError),
    #[error(display = "Bincode serialisation")]
    Bincode(#[source] bincode::Error),
    #[error(display = "Base64 decode ")]
    Base64(#[source] base64::DecodeError),
    #[error(display = "Configuration {}", e)]
    Configuration { e: String },
    #[error(display = "Operation not allowed")]
    OperationNotAllowed,
    #[error(display = "Connection cancelled")]
    ConnectionCancelled,
    #[error(display = "Channel receive error ")]
    ChannelRecv(#[source] mpsc::RecvError),
    #[error(display = "Could not add certificate to PKI")]
    WebPki,
    #[error(display = "Invalid wire message.")]
    InvalidWireMsgFlag,
    #[error(display = "Stream write error ")]
    WriteError(#[source] quinn::WriteError),
    #[error(display = "Read to end error ")]
    ReadToEndError(#[source] quinn::ReadToEndError),
    #[error(display = "Could not add certificate ")]
    AddCertificateError(#[source] quinn::ParseError),
}
