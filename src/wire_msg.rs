// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::{utils, R};
use std::fmt;
use std::net::SocketAddr;

const MAX_MESSAGE_SIZE_FOR_SERIALISATION: usize = 1024; // 1 KiB

/// Final type serialised and sent on the wire by QuicP2p
#[derive(Serialize, Deserialize, Debug)]
pub enum WireMsg {
    Handshake(Handshake),
    EndpointEchoReq,
    EndpointEchoResp(SocketAddr),
    UserMsg(bytes::Bytes),
}

impl Into<bytes::Bytes> for WireMsg {
    fn into(self) -> bytes::Bytes {
        if let WireMsg::UserMsg(ref m) = self {
            if m.len() > MAX_MESSAGE_SIZE_FOR_SERIALISATION {
                return m.clone();
            }
        }

        From::from(unwrap!(bincode::serialize(&self)))
    }
}

impl WireMsg {
    pub fn from_raw(raw: Vec<u8>) -> R<Self> {
        if raw.len() > MAX_MESSAGE_SIZE_FOR_SERIALISATION {
            return Ok(WireMsg::UserMsg(From::from(raw)));
        }

        Ok(bincode::deserialize(&raw)?)
    }
}

impl fmt::Display for WireMsg {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            WireMsg::UserMsg(ref m) => {
                write!(f, "WireMsg::UserMsg({})", utils::bin_data_format(&*m))
            }
            ref w => write!(f, "{}", w),
        }
    }
}

/// Type of Handshake.
///
/// If the peer is a client then we allow a single connection between us, otherwise we will have 2
/// connections with a uni-directional stream in each.
///
/// This information will be given to the user for them to deal with it appropriately.
#[derive(Serialize, Deserialize, Debug)]
pub enum Handshake {
    /// The connecting peer is a node. Certificate is needed for allowing connection back to the
    /// peer
    Node { cert_der: Vec<u8> },
    /// The connecting peer is a client. No need for a reverse connection.
    Client,
}

impl fmt::Display for Handshake {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Handshake::Node { ref cert_der } => write!(
                f,
                "Handshake::Node {{ cert_der: {} }}",
                utils::bin_data_format(cert_der)
            ),
            Handshake::Client => write!(f, "Handshake::Client"),
        }
    }
}
