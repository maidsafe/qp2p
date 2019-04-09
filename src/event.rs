use crate::CrustInfo;
use std::fmt;
use std::net::SocketAddr;

/// Crust Events to the user
pub enum Event {
    ConnectionFailure {
        peer_addr: SocketAddr,
    },
    ConnectedTo {
        crust_info: CrustInfo,
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
            Event::ConnectedTo { ref crust_info } => write!(
                f,
                "Event::ConnectedTo - {}, {:?}",
                crust_info.peer_addr, crust_info.peer_cert_der
            ),
            Event::NewMessage { ref peer_addr, .. } => {
                write!(f, "Event::NewMessage - {}", peer_addr)
            }
            Event::Finish => write!(f, "Event::Finish"),
        }
    }
}
