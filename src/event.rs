use crate::error::QuicP2pError;
use crate::{utils, Peer};
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
        node: SocketAddr,
    },
    /// Connection to this peer failed.
    ConnectionFailure {
        /// Peer.
        peer: Peer,
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

impl Event {
    pub(crate) fn is_node_event(&self) -> bool {
        match self {
            Event::BootstrapFailure => true,
            Event::BootstrappedTo { .. } => true,
            Event::ConnectionFailure { peer, .. }
            | Event::SentUserMessage { peer, .. }
            | Event::UnsentUserMessage { peer, .. }
            | Event::ConnectedTo { peer }
            | Event::NewMessage { peer, .. } => peer.is_node(),
            Event::Finish => true,
        }
    }
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Event::BootstrapFailure => write!(f, "Event::BootstrapFailure"),
            Event::BootstrappedTo { node } => {
                write!(f, "Event::BootstrappedTo {{ node: {} }}", node)
            }
            Event::ConnectionFailure { peer, err } => write!(
                f,
                "Event::ConnectionFailure {{ peer: {}, err: {} }}",
                peer.peer_addr(),
                err
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
