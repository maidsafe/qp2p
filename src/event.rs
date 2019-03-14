use std::net::SocketAddr;

/// Crust Events to the user
#[derive(Serialize, Deserialize, Debug)]
pub enum Event {
    ConnectionFailure { peer: SocketAddr },
    NewMessage { peer: SocketAddr, msg: Vec<u8> },
}
