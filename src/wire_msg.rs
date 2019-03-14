use std::net::SocketAddr;

/// Final type serialised and sent on the wire by Crust
#[derive(Serialize, Deserialize, Debug)]
pub enum WireMsg {
    EndpointEchoReq,
    EndpointEchoResp(SocketAddr),
    UserMsg(Vec<u8>),
}
