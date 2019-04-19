// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::ctx_mut;
use crate::error::Error;
use std::net::SocketAddr;

/// Result used by `QuicP2p`.
pub type R<T> = Result<T, Error>;

/// This is to terminate the connection attempt should it take too long to mature to completeness.
pub type ConnectTerminator = tokio::sync::mpsc::Sender<()>;
/// Obtain a `ConnectTerminator` paired with a corresponding receiver.
pub fn connect_terminator() -> (ConnectTerminator, tokio::sync::mpsc::Receiver<()>) {
    tokio::sync::mpsc::channel(1)
}

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
    debug!(
        "ERROR in communication with peer {}: {:?} - {}. Details: {}",
        peer_addr, e, e, details
    );
    let _ = ctx_mut(|c| c.connections.remove(&peer_addr));
}

#[cfg(test)]
pub mod testing {
    use crate::config::SerialisableCertificate;
    use crate::dirs::{Dirs, OverRide};
    use crate::NodeInfo;
    use rand::Rng;
    use std::env;
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
    use std::path::PathBuf;

    pub fn test_dirs() -> Dirs {
        Dirs::Overide(OverRide::new(&unwrap!(tmp_dir().to_str())))
    }

    pub fn rand_node_info() -> NodeInfo {
        let peer_cert_der = SerialisableCertificate::default().cert_der;
        let mut rng = rand::thread_rng();
        let port: u16 = rng.gen();
        let peer_addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port));
        NodeInfo {
            peer_addr,
            peer_cert_der,
        }
    }

    fn tmp_dir() -> PathBuf {
        let fname = format!("{:016x}.quic_p2p_tests", rand::random::<u64>());
        let mut path = env::temp_dir();
        path.push(fname);
        path
    }
}
