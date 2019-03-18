use std::net::SocketAddr;

/// Crust Events to the user
#[derive(Debug)]
pub enum Event {
    ConnectionFailure {
        peer_addr: SocketAddr,
    },
    ConnectedTo {
        peer_addr: SocketAddr,
    },
    NewMessage {
        peer_addr: SocketAddr,
        msg: Vec<u8>,
    },
    /// No more messages will be fired after this
    // TODO Currently used only for testing
    Finish,
}
