// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::utils;
use std::fmt;
use std::net::SocketAddr;

/// Representation of a peer to us.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub enum Peer {
    /// Stores Node information.
    Node {
        /// Information needed to connect to a node.
        node_info: NodeInfo,
    },
    /// Stores client information.
    Client {
        /// Address of the client reaching us.
        peer_addr: SocketAddr,
    },
}

impl Peer {
    /// Get peer's Endpoint
    pub fn peer_addr(&self) -> SocketAddr {
        match *self {
            Peer::Node { ref node_info } => node_info.peer_addr,
            Peer::Client { peer_addr } => peer_addr,
        }
    }

    /// Get peer's Certificate
    ///
    /// If the peer was a node then the function returns the certificate used by it. For clients it
    /// is not useful in our network to share certificates as we don't reverse connect to the
    /// clients. Due to absence of the knowledge of client's certificate (as it's not exchanged in
    /// handshake) this function retuns `None` for client peers.
    pub fn peer_cert_der(&self) -> Option<&[u8]> {
        match *self {
            Peer::Node { ref node_info } => Some(&node_info.peer_cert_der),
            Peer::Client { .. } => None,
        }
    }
}

/// Information for a peer of type `Peer::Node`.
///
/// This is a necessary information needed to connect to someone.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub struct NodeInfo {
    /// Endpoint of the node
    pub peer_addr: SocketAddr,
    /// Certificate of the node
    pub peer_cert_der: Vec<u8>,
}

impl Into<Peer> for NodeInfo {
    fn into(self) -> Peer {
        Peer::Node { node_info: self }
    }
}

impl fmt::Display for NodeInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "NodeInfo {{ peer_addr: {}, peer_cert_der: {} }}",
            self.peer_addr,
            utils::bin_data_format(&self.peer_cert_der)
        )
    }
}
