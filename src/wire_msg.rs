// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::{
    error::{Error, Result},
    utils,
};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::{fmt, net::SocketAddr};
use unwrap::unwrap;

/// Final type serialised and sent on the wire by QuicP2p
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum WireMsg {
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
    pub fn from_raw(mut raw: Vec<u8>) -> Result<Self> {
        if raw.is_empty() {
            Err(Error::EmptyResponse)
        } else {
            let msg_flag = raw.pop();
            match msg_flag {
                Some(flag) if flag == USER_MSG_FLAG => Ok(WireMsg::UserMsg(From::from(raw))),
                Some(flag) if flag == !USER_MSG_FLAG => Ok(bincode::deserialize(&raw)?),
                _ => Err(Error::InvalidWireMsgFlag),
            }
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
        }
    }
}
