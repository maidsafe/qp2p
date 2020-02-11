// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::{error::QuicP2pError, utils, R};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::{fmt, net::SocketAddr};
use unwrap::unwrap;

/// Final type serialised and sent on the wire by QuicP2p
#[derive(Serialize, Deserialize, Debug)]
pub enum WireMsg {
    Handshake(Handshake),
    EndpointEchoReq,
    EndpointEchoResp(SocketAddr),
    UserMsg(Bytes),
}

const USER_MSG_FLAG: u8 = 0;

impl Into<(Bytes, u8)> for WireMsg {
    fn into(self) -> (Bytes, u8) {
        match self {
            WireMsg::UserMsg(ref m) => (m.clone(), USER_MSG_FLAG),
            _ => (
                From::from(unwrap!(bincode::serialize(&self))),
                !USER_MSG_FLAG,
            ),
        }
    }
}

impl WireMsg {
    pub fn from_raw(mut raw: Vec<u8>) -> R<Self> {
        let msg_flag = raw.pop();

        match msg_flag {
            Some(flag) if flag == USER_MSG_FLAG => Ok(WireMsg::UserMsg(From::from(raw))),
            Some(flag) if flag == !USER_MSG_FLAG => Ok(bincode::deserialize(&raw)?),
            _x => Err(QuicP2pError::InvalidWireMsgFlag),
        }
    }
}

impl fmt::Display for WireMsg {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            WireMsg::UserMsg(ref m) => {
                write!(f, "WireMsg::UserMsg({})", utils::bin_data_format(&*m))
            }
            WireMsg::EndpointEchoReq => write!(f, "WireMsg::EndpointEchoReq"),
            WireMsg::EndpointEchoResp(ref sa) => write!(f, "WireMsg::EndpointEchoResp({})", sa),
            WireMsg::Handshake(ref hs) => write!(f, "WireMsg::Handshake({})", hs),
        }
    }
}

/// Type of Handshake.
///
/// If the peer is a client then we allow a single connection between us. This can have multiple
/// uni-directional streams from either side to the other. For Node-Node however we will have 2
/// connections with each allowing multiple uni-directional streams but only in one direction - the
/// active connection to a peer will allow only outgoing uni-directional streams from it and a
/// passive connection from a peer will allow only incoming uni-directional streams from it.
///
/// Depending on the handshake we will categorise the peer and give this information to the user.
#[derive(Serialize, Deserialize, Debug)]
pub enum Handshake {
    /// The connecting peer is a node.
    Node,
    /// The connecting peer is a client. No need for a reverse connection.
    Client,
}

impl fmt::Display for Handshake {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Handshake::Node => write!(f, "Handshake::Node",),
            Handshake::Client => write!(f, "Handshake::Client"),
        }
    }
}
