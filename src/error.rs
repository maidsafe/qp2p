// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use std::io;
use std::net::SocketAddr;
use std::sync::mpsc;

quick_error! {
    #[derive(Debug)]
     /// Error types encountered during the operation of this library.
     pub enum Error {
         /// Error type associated with I/O operations.
         Io(e: io::Error) {
             display("IO Error: {}", e)
             from()
         }
         /// Errors encountered while reading from a QUIC stream.
         Read(e: quinn::ReadError) {
             display("Read Error: {}", e)
             from()
         }
         /// Error returned when a Bi-directional stream is attempted by a peer.
         BiDirectionalStreamAttempted(peer_addr: SocketAddr) {
             display("Bi-directional stream attempted by peer {}", peer_addr)
         }
         /// Errors encountered while creating a new connection.
         Connect(e: quinn::ConnectError) {
             display("Connection Error: {}", e)
             from()
         }
         /// Errors explaining why an established connection might be lost.
         Connection(e: quinn::ConnectionError) {
             display("Connection Error: {}", e)
             from()
         }
         /// Error while creating a quinn endpoint
         Endpoint(e: quinn::EndpointError) {
             display("Endpoint error: {}", e)
             from()
         }
         /// Error encountered while parsing a certificate or key.
         CertificateParseError(e: String) {
             display("Certificate Parse Error: {}", e)
             from()
         }
         /// Error returned when a duplicate connection to a peer is attempted.
         DuplicateConnectionToPeer(peer_addr: SocketAddr) {
             display("Duplicate connection attempted to peer {}", peer_addr)
         }
         /// Error returned when an endpoint echo server is not found.
         NoEndpointEchoServerFound {
             display("There's no endpoint echo server to ask. Current network configuration")
         }
         /// Error returned when a receive operation on a channel fails.
         OneShotRx(e: tokio::sync::oneshot::error::RecvError) {
             display("Oneshot Receiver error: {}", e)
             from()
         }
         /// Errors encountered while establishing TLS.
         TLS(e: rustls::TLSError) {
             display("TLE error: {}", e)
             from()
         }
         /// Error produced when (de)serialisation is unsuccessful.
         Bincode(e: bincode::Error) {
             display("Bincode error: {}", e)
             from()
         }
         /// Errors encountered while decoding Base64 values.
         Base64(e: base64::DecodeError) {
             display("Base64 decoding error: {}", e)
             from()
         }
         /// Error produced when configuration is not understood.
         Configuration(e: String) {
             display("Configuration error: {}", e)
         }
         /// Forbidden operation attempted
         OperationNotAllowed {
             display("This operation is not allowed for us")
         }
         /// Connection Cancelled
         ConnectionCancelled {
             display("Connection was actively cancelled")
             from()
         }
         /// Failed receiving from an `mpsc::channel`.
         ChannelRecv(e: mpsc::RecvError) {
             display("Channel receive error: {}", e)
             cause(e)
             from()
         }
         /// Could not deserialise a wire message.
         InvalidWireMsgFlag {
             display("Could not deserialise a wire message: unexpected message type flag")
         }
         /// Write error
         WriteError(e: quinn::WriteError) {
             display("Stream Write error: {}", e)
             cause(e)
             from()
         }
         /// Read to end error
         ReadToEndError(e: quinn::ReadToEndError) {
             display("Read to end error: {}", e)
             cause(e)
             from()
         }
         /// Add certificate error
         AddCertificateError(e: String) {
             display("Could not add certificate: {}", e)
         }
     }
}
