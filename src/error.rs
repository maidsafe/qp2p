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
     pub enum Error {
         Io(e: io::Error) {
             display("IO Error: {}", e)
             from()
         }
         Read(e: quinn::ReadError) {
             display("Read Error: {}", e)
             from()
         }
         BiDirectionalStreamAttempted(peer_addr: SocketAddr) {
             display("Bi-directional stream attempted by peer {}", peer_addr)
         }
         Connect(e: quinn::ConnectError) {
             display("Connection Error: {}", e)
             from()
         }
         Connection(e: quinn::ConnectionError) {
             display("Connection Error: {}", e)
             from()
         }
         Endpoint(e: quinn::EndpointError) {
             display("Endpoint error: {}", e)
             from()
         }
         CertificateParseError(e: quinn::tls::ParseError) {
             display("Certificate Parse Error: {}", e)
             from()
         }
         DuplicateConnectionToPeer(peer_addr: SocketAddr) {
             display("Duplicate connection attempted to peer {}", peer_addr)
         }
         NoEndpointEchoServerFound {
             display("There's no endpoint echo server to ask. Current network configuration")
         }
         OneShotRx(e: tokio::sync::oneshot::error::RecvError) {
             display("Oneshot Receiver error: {}", e)
             from()
         }
         TLS(e: rustls::TLSError) {
             display("TLE error: {}", e)
             from()
         }
         Bincode(e: bincode::Error) {
             display("Bincode error: {}", e)
             from()
         }
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
     }
}
