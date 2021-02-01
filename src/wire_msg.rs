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
use log::trace;
use serde::{Deserialize, Serialize};
use std::{fmt, net::SocketAddr};

const MSG_HEADER_LEN: usize = 9;
const MSG_PROTOCOL_VERSION: u16 = 0x0001;

/// Final type serialised and sent on the wire by `QuicP2p`
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum WireMsg {
    EndpointEchoReq,
    EndpointEchoResp(SocketAddr),
    EndpointVerificationReq(SocketAddr),
    EndpointVerficationResp(bool), // Use result here?
    UserMsg(Bytes),
}

const USER_MSG_FLAG: u8 = 0x00;
const ECHO_SRVC_MSG_FLAG: u8 = 0x01;

impl WireMsg {
    // Read a message's bytes from the provided stream
    pub async fn read_from_stream(recv: &mut quinn::RecvStream) -> Result<Self> {
        let mut header_bytes = [0; MSG_HEADER_LEN];
        log::debug!("reading header");
        recv.read_exact(&mut header_bytes).await?;

        let msg_header = MsgHeader::from_bytes(header_bytes);
        log::debug!("reading data of length: {}", msg_header.data_len());
        let mut data: Vec<u8> = vec![0; msg_header.data_len()];
        let msg_flag = msg_header.usr_msg_flag();

        recv.read_exact(&mut data).await?;
        trace!("Got new message with {} bytes.", data.len());

        if data.is_empty() {
            Err(Error::EmptyResponse)
        } else if msg_flag == USER_MSG_FLAG {
            Ok(WireMsg::UserMsg(From::from(data)))
        } else if msg_flag == ECHO_SRVC_MSG_FLAG {
            Ok(bincode::deserialize(&data)?)
        } else {
            Err(Error::InvalidMsgFlag(msg_flag))
        }
    }

    // Helper to write WireMsg bytes to the provided stream.
    pub async fn write_to_stream(&self, send_stream: &mut quinn::SendStream) -> Result<()> {
        // Let's generate the message bytes
        let (msg_bytes, msg_flag) = match self {
            WireMsg::UserMsg(ref m) => (m.clone(), USER_MSG_FLAG),
            _ => (From::from(bincode::serialize(&self)?), ECHO_SRVC_MSG_FLAG),
        };
        trace!("Sending message to remote peer ({} bytes)", msg_bytes.len());

        let msg_header = MsgHeader::new(&msg_bytes, msg_flag)?;
        let header_bytes = msg_header.to_bytes();

        // Send the header bytes over QUIC
        send_stream.write_all(&header_bytes).await?;

        // Send message bytes over QUIC
        send_stream.write_all(&msg_bytes[..]).await?;

        Ok(())
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
            WireMsg::EndpointVerificationReq(ref sa) => {
                write!(f, "WireMsg::EndpointVerificationReq({})", sa)
            }
            WireMsg::EndpointVerficationResp(valid) => write!(
                f,
                "WireMsg::EndpointEchoResp({})",
                if valid { "Valid" } else { "Invalid" }
            ),
        }
    }
}

/// Message Header that is sent over the wire
/// Format of the message header is as follows
/// | version | message length | `usr_msg_flag` | reserved |
/// | 2 bytes |     4 bytes    |    1 byte    | 2 bytes  |
#[derive(Debug)]
struct MsgHeader {
    version: u16,
    data_len: usize,
    usr_msg_flag: u8,
    #[allow(unused)]
    reserved: [u8; 2],
}

impl MsgHeader {
    pub fn new(msg: &Bytes, usr_msg_flag: u8) -> Result<Self> {
        let data_len = msg.len();
        if data_len > u32::MAX as usize {
            return Err(Error::MaxLengthExceeded(data_len));
        }
        Ok(Self {
            version: MSG_PROTOCOL_VERSION,
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

    pub fn to_bytes(&self) -> [u8; MSG_HEADER_LEN] {
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

    pub fn from_bytes(bytes: [u8; MSG_HEADER_LEN]) -> Self {
        let version = u16::from_be_bytes([bytes[0], bytes[1]]);
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
