use crate::{utils, Peer};
use std::fmt;
use std::net::SocketAddr;

/// Crust Events to the user
#[derive(Debug)]
pub enum Event {
    ConnectionFailure {
        peer_addr: SocketAddr,
    },
    ConnectedTo {
        peer: Peer,
    },
    NewMessage {
        peer_addr: SocketAddr,
        msg: bytes::Bytes,
    },
    /// No more messages will be fired after this
    // TODO Currently used only for testing
    Finish,
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Event::NewMessage {
                ref peer_addr,
                ref msg,
            } => write!(
                f,
                "Event::NewMessage {{ peer_addr: {}, msg: {} }}",
                peer_addr,
                utils::bin_data_format(&*msg)
            ),
            ref blah => write!(f, "{}", blah),
        }
    }
}
