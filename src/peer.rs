// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use serde::{Deserialize, Serialize};
use std::{fmt, net::SocketAddr};

/// Representation of a peer to us.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub enum Peer {
    /// Peer is a node.
    Node(SocketAddr),
    /// Peer is a client.
    Client(SocketAddr),
}

impl Peer {
    /// Get peer's Endpoint
    pub fn peer_addr(&self) -> SocketAddr {
        match *self {
            Self::Node(addr) | Self::Client(addr) => addr,
        }
    }

    pub(crate) fn is_node(&self) -> bool {
        match *self {
            Peer::Node { .. } => true,
            Peer::Client { .. } => false,
        }
    }
}

impl fmt::Display for Peer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Node(addr) => write!(f, "Peer::Node({})", addr),
            Self::Client(addr) => write!(f, "Peer::Client({})", addr),
        }
    }
}
