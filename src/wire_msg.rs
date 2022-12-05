// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::{
    error::{RecvError, SendError},
    utils,
};
use bytes::{Bytes, BytesMut};
use futures::TryFutureExt;
use serde::{Deserialize, Serialize};
use std::convert::TryFrom;
use std::{fmt, net::SocketAddr};

const MSG_HEADER_LEN: usize = 17;
const MSG_PROTOCOL_VERSION: u16 = 0x0002;

/// The message bytes that comprise the message,
/// These are (header, dst, payload) bytes to be passed to qp2p
pub type UsrMsgBytes = (Bytes, Bytes, Bytes);

/// Final type serialised and sent on the wire by `QuicP2p`
#[derive(Serialize, Deserialize, Debug, Clone)]
pub(crate) enum WireMsg {
    EndpointEchoReq,
    EndpointEchoResp(SocketAddr),
    EndpointVerificationReq(SocketAddr),
    EndpointVerificationResp(bool),
    /// Msg Header, Dst and Payload bytes
    /// Reagularly we send the same header and payload, with a new dst
    UserMsg(UsrMsgBytes),
}

const USER_MSG_FLAG: u8 = 0x00;
const ECHO_SRVC_MSG_FLAG: u8 = 0x01;

impl WireMsg {
    // Read a message's bytes from the provided stream
    pub(crate) async fn read_from_stream(
        recv: &mut quinn::RecvStream,
    ) -> Result<Option<Self>, RecvError> {
        let mut header_bytes = [0; MSG_HEADER_LEN];
        match recv.read(&mut header_bytes).err_into().await {
            Err(error) => return Err(error),
            Ok(None) => return Ok(None),
            Ok(Some(len)) => {
                if len < MSG_HEADER_LEN {
                    recv.read_exact(&mut header_bytes[len..]).await?;
                }
            }
        }

        let msg_header = MsgHeader::from_bytes(header_bytes);
        // https://github.com/rust-lang/rust/issues/70460 for work on a cleaner alternative:
        #[cfg(not(any(target_pointer_width = "32", target_pointer_width = "64")))]
        {
            compile_error!("You need an architecture capable of addressing 32-bit pointers");
        }
        // we know we can convert without loss thanks to our assertions above
        let header_length = msg_header.user_header_len() as usize;
        let dst_length = msg_header.user_dst_len() as usize;
        let payload_length = msg_header.user_payload_len() as usize;

        let mut header_data = BytesMut::with_capacity(header_length);
        let mut dst_data = BytesMut::with_capacity(dst_length);
        let mut payload_data = BytesMut::with_capacity(payload_length);
        // buffer capacity does not actually give us length, so this sets us up
        header_data.resize(header_length, 0);
        dst_data.resize(dst_length, 0);
        payload_data.resize(payload_length, 0);

        let msg_flag = msg_header.usr_msg_flag();

        // fill up our data vecs from the stream
        recv.read_exact(&mut header_data).await?;
        recv.read_exact(&mut dst_data).await?;
        recv.read_exact(&mut payload_data).await?;

        if payload_data.is_empty() {
            Err(RecvError::EmptyMsgPayload)
        } else if msg_flag == USER_MSG_FLAG {
            Ok(Some(WireMsg::UserMsg((
                header_data.freeze(),
                dst_data.freeze(),
                payload_data.freeze(),
            ))))
        } else if msg_flag == ECHO_SRVC_MSG_FLAG {
            Ok(Some(bincode::deserialize(&payload_data)?))
        } else {
            Err(RecvError::InvalidMsgTypeFlag(msg_flag))
        }
    }

    // Helper to write WireMsg bytes to the provided stream.
    pub(crate) async fn write_to_stream(
        &self,
        send_stream: &mut quinn::SendStream,
    ) -> Result<(), SendError> {
        // Let's generate the message bytes
        let ((msg_head, msg_dst, msg_payload), msg_flag) = match self {
            WireMsg::UserMsg((ref header, ref dst, ref payload)) => (
                (header.clone(), dst.clone(), payload.clone()),
                USER_MSG_FLAG,
            ),
            _ => (
                (
                    BytesMut::zeroed(4).freeze(), // at least 4 bytes reserved here to aid deserialisation
                    BytesMut::zeroed(4).freeze(), // at least 4 bytes reserved here to aid deserialisation
                    Bytes::from(bincode::serialize(&self)?),
                ),
                ECHO_SRVC_MSG_FLAG,
            ),
        };

        let msg_header = MsgHeader::new(
            msg_head.clone(),
            msg_dst.clone(),
            msg_payload.clone(),
            msg_flag,
        )?;

        let header_bytes = msg_header.to_bytes();

        // Send the header bytes over QUIC
        send_stream.write_all(&header_bytes).await?;

        // Send message bytes over QUIC
        send_stream.write_all(&msg_head).await?;
        send_stream.write_all(&msg_dst).await?;
        send_stream.write_all(&msg_payload).await?;

        Ok(())
    }
}

impl fmt::Display for WireMsg {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            WireMsg::UserMsg((ref h, ref d, ref p)) => {
                write!(
                    f,
                    "WireMsg::UserMsg({} {} {})",
                    utils::bin_data_format(h),
                    utils::bin_data_format(d),
                    utils::bin_data_format(p)
                )
            }
            WireMsg::EndpointEchoReq => write!(f, "WireMsg::EndpointEchoReq"),
            WireMsg::EndpointEchoResp(ref sa) => write!(f, "WireMsg::EndpointEchoResp({})", sa),
            WireMsg::EndpointVerificationReq(ref sa) => {
                write!(f, "WireMsg::EndpointVerificationReq({})", sa)
            }
            WireMsg::EndpointVerificationResp(valid) => write!(
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
    user_header_len: u32,
    user_dst_len: u32,
    payload_len: u32,
    usr_msg_flag: u8,
    #[allow(unused)]
    reserved: [u8; 2],
}

impl MsgHeader {
    fn new(
        user_header: Bytes,
        user_dst: Bytes,
        payload: Bytes,
        usr_msg_flag: u8,
    ) -> Result<Self, SendError> {
        let total_len = user_header.len() + user_dst.len() + payload.len();
        let _total_len =
            u32::try_from(total_len).map_err(|_| SendError::MessageTooLong(total_len))?;

        Ok(Self {
            version: MSG_PROTOCOL_VERSION,
            user_header_len: user_header.len() as u32,
            user_dst_len: user_dst.len() as u32,
            payload_len: payload.len() as u32,
            usr_msg_flag,
            reserved: [0, 0],
        })
    }

    fn user_header_len(&self) -> u32 {
        self.user_header_len
    }
    fn user_dst_len(&self) -> u32 {
        self.user_dst_len
    }
    fn user_payload_len(&self) -> u32 {
        self.payload_len
    }

    fn usr_msg_flag(&self) -> u8 {
        self.usr_msg_flag
    }

    fn to_bytes(&self) -> [u8; MSG_HEADER_LEN] {
        let version = self.version.to_be_bytes();
        let user_header_len = self.user_header_len.to_be_bytes();
        let user_dst_len = self.user_dst_len.to_be_bytes();
        let user_payload_len = self.payload_len.to_be_bytes();
        [
            version[0],
            version[1],
            user_header_len[0],
            user_header_len[1],
            user_header_len[2],
            user_header_len[3],
            user_dst_len[0],
            user_dst_len[1],
            user_dst_len[2],
            user_dst_len[3],
            user_payload_len[0],
            user_payload_len[1],
            user_payload_len[2],
            user_payload_len[3],
            self.usr_msg_flag,
            0,
            0,
        ]
    }

    fn from_bytes(bytes: [u8; MSG_HEADER_LEN]) -> Self {
        let version = u16::from_be_bytes([bytes[0], bytes[1]]);
        let user_header_len = u32::from_be_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]);
        let user_dst_len = u32::from_be_bytes([bytes[6], bytes[7], bytes[8], bytes[9]]);
        let user_payload_len = u32::from_be_bytes([bytes[10], bytes[11], bytes[12], bytes[13]]);
        let usr_msg_flag = bytes[14];
        Self {
            version,
            user_header_len,
            user_dst_len,
            payload_len: user_payload_len,
            usr_msg_flag,
            reserved: [0, 0],
        }
    }
}
