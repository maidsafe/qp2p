use crate::{utils, NodeInfo, Peer};
use std::fmt;
use std::net::SocketAddr;

/// QuicP2p Events to the user
#[derive(Debug)]
pub enum Event {
    /// Network bootstrap failed.
    BootstrapFailure,
    /// Bootstrap connection to this node was successful.
    BootstrappedTo {
        /// Node information.
        node: NodeInfo,
    },
    /// Connection to this peer failed.
    ConnectionFailure {
        /// Peer address.
        peer_addr: SocketAddr,
    },
    /// The given message was not sent to this peer.
    UnsentUserMessage {
        /// Peer address.
        peer_addr: SocketAddr,
        /// Unsent message.
        msg: bytes::Bytes,
    },
    /// Successfully connected to this peer.
    ConnectedTo {
        /// Peer information.
        peer: Peer,
    },
    /// A new message was received from this peer.
    NewMessage {
        /// Sending peer address.
        peer_addr: SocketAddr,
        /// The new message.
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
