// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::config::{Config, SerialisableCertificate};
use crate::connection::{FromPeer, QConn, ToPeer};
use crate::context::ctx;
use crate::dirs::{Dirs, OverRide};
use crate::event::Event;
use crate::wire_msg::WireMsg;
use crate::{communicate, Builder, NodeInfo, Peer, QuicP2p};
use rand::Rng;
use std::collections::HashSet;
use std::env;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use tokio::prelude::Future;
use tokio::runtime::current_thread;

/// Extend `QuicP2p` with test functions.
impl QuicP2p {
    /// Send an arbitrary message. Used for testing malicious nodes and clients.
    pub(crate) fn send_wire_msg(&mut self, peer: Peer, msg: WireMsg) {
        self.el.post(move || {
            communicate::try_write_to_peer(peer, msg);
        });
    }
}

pub(crate) fn test_dirs() -> Dirs {
    Dirs::Overide(OverRide::new(&unwrap!(tmp_rand_dir().to_str())))
}

pub(crate) fn rand_node_info() -> NodeInfo {
    let peer_cert_der = SerialisableCertificate::default().cert_der;
    let mut rng = rand::thread_rng();
    let port: u16 = rng.gen();
    let peer_addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port));
    NodeInfo {
        peer_addr,
        peer_cert_der,
    }
}

fn tmp_rand_dir() -> PathBuf {
    let fname = format!("quic_p2p_tests_{:016x}", rand::random::<u64>());
    let mut path = env::temp_dir();
    path.push(fname);
    path
}

/// Creates a new `QuicP2p` instance for testing.
pub(crate) fn new_random_qp2p_for_unit_test(
    is_addr_unspecified: bool,
    contacts: HashSet<NodeInfo>,
) -> (QuicP2p, Receiver<Event>) {
    let (tx, rx) = mpsc::channel();
    let qp2p = {
        let mut cfg = Config::with_default_cert();
        cfg.hard_coded_contacts = contacts;
        cfg.port = Some(0);
        if !is_addr_unspecified {
            cfg.ip = Some(IpAddr::V4(Ipv4Addr::LOCALHOST));
        }
        unwrap!(Builder::new(tx).with_config(cfg).build())
    };

    (qp2p, rx)
}

/// Connect and open a bi-directional stream.
/// This will fail if we don't have a connection to the peer or if the peer is in an invalid state
/// to be sent a message to.
pub(crate) fn write_to_bi_stream(peer_addr: SocketAddr, wire_msg: WireMsg) {
    fn write_to_bi(conn: &QConn, wire_msg: WireMsg) {
        let leaf = conn
            .open_bi()
            .map_err(move |e| panic!("Open-Bidirectional: {:?} {}", e, e))
            .and_then(move |(o_stream, _i_stream)| {
                tokio::io::write_all(o_stream, wire_msg.into())
                    .map_err(move |e| panic!("Write-All: {:?} {}", e, e))
            })
            .and_then(move |(o_stream, _): (_, bytes::Bytes)| {
                tokio::io::shutdown(o_stream)
                    .map_err(move |e| panic!("Shutdown-after-write: {:?} {}", e, e))
            })
            .map(|_| ());
        current_thread::spawn(leaf);
    }

    ctx(|c| {
        let conn = match c.connections.get(&peer_addr) {
            Some(conn) => conn,
            None => panic!("Asked to communicate with an unknown peer: {}", peer_addr),
        };

        match &conn.to_peer {
            ToPeer::NotNeeded => {
                if let FromPeer::Established { ref q_conn, .. } = conn.from_peer {
                    write_to_bi(q_conn, wire_msg);
                } else {
                    panic!(
                        "TODO We cannot communicate with someone we are not needing to connect to \
                         and they are not connected to us just now. Peer: {}",
                        peer_addr
                    );
                }
            }
            ToPeer::Established { ref q_conn, .. } => write_to_bi(q_conn, wire_msg),
            ToPeer::NoConnection | ToPeer::Initiated { .. } => {
                panic!(
                    "Peer {} is in invalid state {:?} to be communicated to",
                    peer_addr, conn.to_peer
                );
            }
        }
    })
}
