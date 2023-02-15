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

use serde::{Deserialize, Serialize};
use std::fmt;
use std::{convert::TryFrom, time::Instant};
use tracing::trace;
const MSG_HEADER_LEN: usize = 16;
const MSG_PROTOCOL_VERSION: u16 = 0x0002;

/// Final type serialised and sent on the wire by `QuicP2p`
/// The message bytes that comprise the message,
/// These are (header, dst, payload) bytes to be passed to qp2p
#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
pub struct WireMsg(pub UsrMsgBytes);

/// The message bytes that comprise the message,
/// These are (header, dst, payload) bytes to be passed to qp2p
pub type UsrMsgBytes = (Bytes, Bytes, Bytes);

impl WireMsg {
    // Read a message's bytes from the provided stream
    /// # Cancellation safety
    /// Warning: This method is not cancellation safe!
    pub(crate) async fn read_from_stream(mut recv: quinn::RecvStream) -> Result<Self, RecvError> {
        let mut header_bytes = [0; MSG_HEADER_LEN];
        recv.read_exact(&mut header_bytes).await?;

        let msg_header = MsgHeader::from_bytes(header_bytes);

        let start = Instant::now();
        let all_bytes = recv.read_to_end(1024 * 1024 * 100).await?;

        let duration = start.elapsed();
        trace!(
            "Incoming new msg. Reading {:?} bytes took: {:?}",
            all_bytes.len(),
            duration
        );

        if all_bytes.is_empty() {
            return Err(RecvError::EmptyMsgPayload);
        }

        let mut bytes = Bytes::from(all_bytes);

        // https://github.com/rust-lang/rust/issues/70460 for work on a cleaner alternative:
        #[cfg(not(any(target_pointer_width = "32", target_pointer_width = "64")))]
        {
            compile_error!("You need an architecture capable of addressing 32-bit pointers");
        }
        // we know we can convert without loss thanks to our assertions above
        let header_length = msg_header.user_header_len() as usize;
        let dst_length = msg_header.user_dst_len() as usize;
        let payload_length = msg_header.user_payload_len() as usize;

        // Check we have all the data and we weren't cut short, otherwise
        // the following would panic...
        if bytes.len() != (header_length + dst_length + payload_length) {
            return Err(RecvError::NotEnoughBytes);
        }

        let header_data = bytes.split_to(header_length);

        let dst_data = bytes.split_to(dst_length);
        let payload_data = bytes;

        if payload_data.is_empty() {
            Err(RecvError::EmptyMsgPayload)
        } else {
            Ok(WireMsg((header_data, dst_data, payload_data)))
        }
    }

    // Helper to write WireMsg bytes to the provided stream.
    pub(crate) async fn write_to_stream(
        &self,
        send_stream: &mut quinn::SendStream,
    ) -> Result<(), SendError> {
        // Let's generate the message bytes
        let WireMsg((ref msg_head, ref msg_dst, ref msg_payload)) = self;

        let msg_header = MsgHeader::new(msg_head.clone(), msg_dst.clone(), msg_payload.clone())?;

        let header_bytes = msg_header.to_bytes();

        let mut all_bytes = BytesMut::with_capacity(
            header_bytes.len()
                + msg_header.user_header_len() as usize
                + msg_header.user_dst_len() as usize
                + msg_header.user_payload_len() as usize,
        );

        all_bytes.extend_from_slice(&header_bytes);
        all_bytes.extend_from_slice(msg_head);
        all_bytes.extend_from_slice(msg_dst);
        all_bytes.extend_from_slice(msg_payload);

        let start = Instant::now();
        // Send the header bytes over QUIC
        send_stream.write_all(&all_bytes).await?;
        let duration = start.elapsed();
        trace!("Writing {:?} bytes took: {duration:?}", all_bytes.len());

        Ok(())
    }
}

impl fmt::Display for WireMsg {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "WireMsg({} {} {})",
            utils::bin_data_format(&self.0 .0),
            utils::bin_data_format(&self.0 .1),
            utils::bin_data_format(&self.0 .2)
        )
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
    #[allow(unused)]
    reserved: [u8; 2],
}

impl MsgHeader {
    fn new(user_header: Bytes, user_dst: Bytes, payload: Bytes) -> Result<Self, SendError> {
        let total_len = user_header.len() + user_dst.len() + payload.len();
        let _total_len =
            u32::try_from(total_len).map_err(|_| SendError::MessageTooLong(total_len))?;

        Ok(Self {
            version: MSG_PROTOCOL_VERSION,
            user_header_len: user_header.len() as u32,
            user_dst_len: user_dst.len() as u32,
            payload_len: payload.len() as u32,
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
            0,
            0,
        ]
    }

    fn from_bytes(bytes: [u8; MSG_HEADER_LEN]) -> Self {
        let version = u16::from_be_bytes([bytes[0], bytes[1]]);
        let user_header_len = u32::from_be_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]);
        let user_dst_len = u32::from_be_bytes([bytes[6], bytes[7], bytes[8], bytes[9]]);
        let user_payload_len = u32::from_be_bytes([bytes[10], bytes[11], bytes[12], bytes[13]]);
        Self {
            version,
            user_header_len,
            user_dst_len,
            payload_len: user_payload_len,
            reserved: [0, 0],
        }
    }
}
