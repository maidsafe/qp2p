use crate::ctx_mut;
use crate::error::Error;
use std::net::SocketAddr;

/// Convert binary data to a diplay-able format
#[inline]
pub fn bin_data_format(data: &[u8]) -> String {
    let len = data.len();
    if len < 8 {
        return format!("[ {:?} ]", data);
    }

    format!(
        "[ {:02x} {:02x} {:02x} {:02x}..{:02x} {:02x} {:02x} {:02x} ]",
        data[0],
        data[1],
        data[2],
        data[3],
        data[len - 4],
        data[len - 3],
        data[len - 2],
        data[len - 1]
    )
}

/// Handle error in communication
#[inline]
pub fn handle_communication_err(peer_addr: SocketAddr, e: &Error, details: &str) {
    println!(
        "ERROR in communication with peer {}: {:?} - {}. Details: {}",
        peer_addr, e, e, details
    );
    let _ = ctx_mut(|c| c.connections.remove(&peer_addr));
}
