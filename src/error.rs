use std::io;
use std::net::SocketAddr;

quick_error! {
    #[derive(Debug)]
     pub enum Error {
         IoError(e: io::Error) {
             display("IO Error: {}", e)
             from()
         }
         ReadError(e: quinn::ReadError) {
             display("Read Error: {}", e)
             from()
         }
         BiDirectionalStreamAttempted(peer_addr: SocketAddr) {
             display("Bi-directional stream attempted by peer {}", peer_addr)
         }
         ConnectError(e: quinn::ConnectError) {
             display("Connection Error: {}", e)
             from()
         }
         ConnectionError(e: quinn::ConnectionError) {
             display("Connection Error: {}", e)
             from()
         }
         EndpointError(e: quinn::EndpointError) {
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
         TLSError(e: rustls::TLSError) {
             display("TLE error: {}", e)
             from()
         }
         BincodeError(e: bincode::Error) {
             display("Bincode error: {}", e)
             from()
         }
     }
}
