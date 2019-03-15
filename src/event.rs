use std::net::SocketAddr;

/// Crust Events to the user
#[derive(Serialize, Deserialize, Debug)]
pub enum Event {
    ConnectionFailure { peer_addr: SocketAddr },
    ConnectedTo { peer_addr: SocketAddr },
    NewMessage { peer_addr: SocketAddr, msg: Vec<u8> },
}
