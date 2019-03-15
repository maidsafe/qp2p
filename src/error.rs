use std::net::SocketAddr;

quick_error! {
    #[derive(Debug)]
     pub enum Error {
         ConnectError(e: quinn::ConnectError) {
             display("Connection Error: {}", e)
             from()
         }
         ConnectionError(e: quinn::ConnectionError) {
             display("Connection Error: {}", e)
             from()
         }
         DuplicateConnectionToPeer(peer_addr: SocketAddr) {
             display("Duplicate connection attempted to peer {}", peer_addr)
         }
     }
}
