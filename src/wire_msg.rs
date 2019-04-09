use crate::R;
use std::net::SocketAddr;

const MAX_MESSAGE_SIZE_FOR_SERIALISATION: usize = 1024; // 1 KiB

/// Final type serialised and sent on the wire by Crust
#[derive(Serialize, Deserialize, Debug)]
pub enum WireMsg {
    CertificateDer(Vec<u8>),
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
