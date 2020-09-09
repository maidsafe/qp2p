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

pub(crate) const HEADER_LEN: usize = 9;
pub(crate) const VERSION: i16 = 0;
pub(crate) const MAX_DATA_LEN: usize = usize::from_be_bytes([0, 0, 0, 0, 255, 255, 255, 255]);

/// Message Header that is sent over the wire
/// Format of the message header is as follows
/// | version | message length | usr_msg_flag | reserved |
/// | 2 bytes |     4 bytes    |    1 byte    | 2 bytes  |
pub(crate) struct MsgHeader {
    version: i16,
    data_len: usize,
    usr_msg_flag: u8,
    #[allow(unused)]
    reserved: [u8; 2],
}

impl MsgHeader {
    pub fn new(msg: &Bytes, usr_msg_flag: u8) -> Result<Self> {
        let data_len = msg.len();
        if data_len > MAX_DATA_LEN {
            return Err(Error::MaxLengthExceeded);
        }
        Ok(Self {
            version: VERSION,
            data_len,
            usr_msg_flag,
            reserved: [0, 0],
        })
    }

    pub fn data_len(&self) -> usize {
        self.data_len
    }

    pub fn usr_msg_flag(&self) -> u8 {
        self.usr_msg_flag
    }

    pub fn to_bytes(&self) -> [u8; HEADER_LEN] {
        let version = self.version.to_be_bytes();
        let data_len = self.data_len.to_be_bytes();
        [
            version[0],
            version[1],
            data_len[4],
            data_len[5],
            data_len[6],
            data_len[7],
            self.usr_msg_flag,
            0,
            0,
        ]
    }

    pub fn from_bytes(bytes: [u8; HEADER_LEN]) -> Self {
        let version = i16::from_be_bytes([bytes[0], bytes[1]]);
        let data_len = usize::from_be_bytes([0, 0, 0, 0, bytes[2], bytes[3], bytes[4], bytes[5]]);
        let usr_msg_flag = bytes[6];
        Self {
            version,
            data_len,
            usr_msg_flag,
            reserved: [0, 0],
        }
    }
}

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
    pub fn from_raw(raw: Vec<u8>, msg_flag: u8) -> Result<Self> {
        if raw.is_empty() {
            Err(Error::EmptyResponse)
        } else if msg_flag == USER_MSG_FLAG {
            Ok(WireMsg::UserMsg(From::from(raw)))
        } else if msg_flag == !USER_MSG_FLAG {
            Ok(bincode::deserialize(&raw)?)
        } else {
            Err(Error::InvalidWireMsgFlag)
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
