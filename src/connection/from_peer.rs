// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::connection::QConn;
use crate::wire_msg::WireMsg;
use std::fmt;

/// Represent various stages of connection from the peer to us.
pub enum FromPeer {
    NoConnection,
    NotNeeded,
    Established {
        q_conn: QConn,
        pending_reads: Vec<WireMsg>,
    },
}

impl FromPeer {
    pub fn is_not_needed(&self) -> bool {
        if let FromPeer::NotNeeded = *self {
            true
        } else {
            false
        }
    }

    pub fn is_no_connection(&self) -> bool {
        if let FromPeer::NoConnection = *self {
            true
        } else {
            false
        }
    }

    pub fn is_established(&self) -> bool {
        if let FromPeer::Established { .. } = *self {
            true
        } else {
            false
        }
    }
}

impl Default for FromPeer {
    fn default() -> Self {
        FromPeer::NoConnection
    }
}

impl fmt::Debug for FromPeer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            FromPeer::Established {
                ref pending_reads, ..
            } => write!(
                f,
                "FromPeer::Established with {} pending reads",
                pending_reads.len()
            ),
            FromPeer::NoConnection => write!(f, "FromPeer::NoConnection"),
            FromPeer::NotNeeded => write!(f, "FromPeer::NotNeeded"),
        }
    }
}
