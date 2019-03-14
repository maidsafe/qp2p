use std::net::SocketAddr;

/// Crust configurations
#[derive(Default)]
pub struct Config {
    /// Hard Coded contacts
    pub hard_coded_contacts: Vec<SocketAddr>,
    /// Port we want to reserve for QUIC
    pub port: Option<u16>,
}
