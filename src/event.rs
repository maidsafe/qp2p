use crate::PeerInfo;
use std::fmt;
use std::net::SocketAddr;

/// Peer sending Events to the user
pub enum Event {
    ConnectionFailure {
        peer_addr: SocketAddr,
    },
    ConnectedTo {
        peer_info: PeerInfo,
    },
    NewMessage {
        peer_addr: SocketAddr,
        msg: bytes::Bytes,
    },
    /// No more messages will be fired after this
    // TODO Currently used only for testing
    Finish,
}

impl fmt::Debug for Event {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Event::ConnectionFailure { peer_addr } => {
                write!(f, "Event::ConnectionFailure - {}", peer_addr)
            }
            Event::ConnectedTo { ref peer_info } => write!(
                f,
                "Event::ConnectedTo - {}, {:?}",
                peer_info.peer_addr, peer_info.peer_cert_der
            ),
            Event::NewMessage { ref peer_addr, .. } => {
                write!(f, "Event::NewMessage - {}", peer_addr)
            }
            Event::Finish => write!(f, "Event::Finish"),
        }
    }
}
