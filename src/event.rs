use crate::error::QuicP2pError;
use crate::{utils, NodeInfo, Peer};
use std::{fmt, net::SocketAddr};
use utils::Token;

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
        /// Error explaining connection failure.
        err: QuicP2pError,
    },
    /// The given message was successfully sent to this peer.
    SentUserMessage {
        /// Peer.
        peer: Peer,
        /// Sent message.
        msg: bytes::Bytes,
        /// Token, originally given by the user, for context.
        token: Token,
    },
    /// The given message was not sent to this peer.
    UnsentUserMessage {
        /// Peer.
        peer: Peer,
        /// Unsent message.
        msg: bytes::Bytes,
        /// Token, originally given by the user, for context.
        token: Token,
    },
    /// Successfully connected to this peer.
    ConnectedTo {
        /// Peer information.
        peer: Peer,
    },
    /// A new message was received from this peer.
    NewMessage {
        /// Sending peer.
        peer: Peer,
        /// The new message.
        msg: bytes::Bytes,
    },
    /// No more messages will be fired after this
    // TODO Currently used only for testing
    Finish,
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Event::BootstrapFailure => write!(f, "Event::BootstrapFailure"),
            Event::BootstrappedTo { node } => {
                write!(f, "Event::BootstrappedTo {{ node: {} }}", node)
            }
            Event::ConnectionFailure { peer_addr, err } => write!(
                f,
                "Event::ConnectionFailure {{ peer_addr: {}, err: {} }}",
                peer_addr, err
            ),
            Event::SentUserMessage { peer, msg, token } => write!(
                f,
                "Event::SentUserMessage {{ peer: {}, msg: {}, token: {} }}",
                peer.peer_addr(),
                utils::bin_data_format(&*msg),
                token
            ),
            Event::UnsentUserMessage { peer, msg, token } => write!(
                f,
                "Event::UnsentUserMessage {{ peer: {}, msg: {}, token: {} }}",
                peer.peer_addr(),
                utils::bin_data_format(&*msg),
                token
            ),
            Event::ConnectedTo { peer } => write!(f, "Event::ConnectedTo {{ peer: {} }}", peer),
            Event::NewMessage { peer, msg } => write!(
                f,
                "Event::NewMessage {{ peer: {}, msg: {} }}",
                peer.peer_addr(),
                utils::bin_data_format(&*msg)
            ),
            Event::Finish => write!(f, "Event::Finish"),
        }
    }
}
