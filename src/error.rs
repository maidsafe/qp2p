use std::net::SocketAddr;

quick_error! {
    #[derive(Debug)]
     pub enum Error {
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
         CertificateParseError(e: quinn::tls::ParseError) {
             display("Certificate Parse Error: {}", e)
             from()
         }
         DuplicateConnectionToPeer(peer_addr: SocketAddr) {
             display("Duplicate connection attempted to peer {}", peer_addr)
         }
         NoEndpointEchoServerFound {
             display("There's no endpoint echo server to ask.")
         }
     }
}
