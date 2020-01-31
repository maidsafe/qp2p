// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

mod network;
mod node;
#[cfg(test)]
mod tests;

pub use self::network::Network;

use self::node::Node;
use bytes::Bytes;
use crossbeam_channel::Sender;
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    collections::HashSet,
    iter,
    net::{IpAddr, SocketAddr},
    rc::Rc,
    str::FromStr,
};
use structopt::StructOpt;

pub type Token = u64;

/// Builder for `QuickP2p`.
pub struct Builder {
    event_tx: Sender<Event>,
    config: Option<Config>,
}

impl Builder {
    /// New `Builder`
    pub fn new(event_tx: Sender<Event>) -> Self {
        Self {
            event_tx,
            config: Default::default(),
        }
    }

    /// Configuration for `QuicP2p`.
    /// If not specified, will use `Config::default()`.
    pub fn with_config(mut self, config: Config) -> Self {
        self.config = Some(config);
        self
    }

    /// Construct `QuicP2p` with supplied parameters earlier, ready to be used.
    pub fn build(self) -> Result<QuicP2p, Error> {
        Ok(QuicP2p::new(
            self.event_tx,
            self.config.unwrap_or_else(Config::default),
        ))
    }
}

/// Main QuicP2p interface.
pub struct QuicP2p {
    inner: Rc<RefCell<Node>>,
}

impl QuicP2p {
    /// Bootstrap to the network.
    ///
    /// Bootstrap concept is different from "connect" in several ways: `bootstrap()` will try to
    /// connect to all peers which are specified in the config (`hard_coded_contacts`) or were
    /// previously cached. If one bootstrap connection succeeds, all other connections will be dropped.
    ///
    /// In case of success `Event::BootstrapedTo` will be fired. On error quic-p2p will fire `Event::BootstrapFailure`.
    pub fn bootstrap(&mut self) {
        self.inner.borrow_mut().bootstrap()
    }

    /// Connect to the given peer. This will error out if the peer is already in the process of
    /// being connected to OR for any other connection failure reasons.
    pub fn connect_to(&mut self, peer_info: NodeInfo) {
        self.inner.borrow().connect(peer_info.peer_addr);
    }

    /// Disconnect from the given peer
    pub fn disconnect_from(&mut self, peer_addr: SocketAddr) {
        self.inner.borrow_mut().disconnect(peer_addr)
    }

    /// Send message to peer.
    ///
    /// If the peer is not connected, it will attempt to connect to it first
    /// and then send the message. This can be called multiple times while the peer is still being
    /// connected to - all the sends will be buffered until the peer is connected to.
    pub fn send(&mut self, peer: Peer, msg: Bytes, token: Token) {
        self.inner.borrow_mut().send(peer.peer_addr(), msg, token)
    }

    /// Get our connection info to give to others for them to connect to us
    pub fn our_connection_info(&mut self) -> Result<NodeInfo, Error> {
        self.inner.borrow().our_connection_info()
    }

    /// Retrieves current node bootstrap cache.
    pub fn bootstrap_cache(&mut self) -> Result<Vec<NodeInfo>, Error> {
        Ok(self.inner.borrow().bootstrap_cache())
    }

    /// Check whether the given contact is hard-coded (always `true` in mock).
    pub fn is_hard_coded_contact(&self, _node_info: &NodeInfo) -> bool {
        true
    }

    /// Returns the config used to create this instance.
    pub fn config(&self) -> Config {
        self.inner.borrow().config().clone()
    }

    fn new(event_tx: Sender<Event>, config: Config) -> Self {
        Self {
            inner: Node::new(event_tx, config),
        }
    }
}

#[cfg(test)]
impl QuicP2p {
    fn addr(&self) -> SocketAddr {
        *self.inner.borrow().addr()
    }

    fn our_type(&self) -> OurType {
        self.inner.borrow().our_type()
    }
}

/// Configuration for `QuicP2p`.
#[derive(Debug, Default, Clone, Eq, PartialEq, Serialize, Deserialize, StructOpt)]
pub struct Config {
    /// Hard-coded contacts.
    #[structopt(
        short,
        long,
        default_value = "[]",
        parse(try_from_str = "serde_json::from_str")
    )]
    pub hard_coded_contacts: HashSet<NodeInfo>,
    /// Type of our `QuicP2p` instance: node or client.
    #[structopt(short = "t", long, default_value = "node")]
    pub our_type: OurType,
    /// Port to listen to.
    pub ip: Option<IpAddr>,
    /// IP address to listen to.
    pub port: Option<u16>,
}

impl Config {
    /// Create `Config` for node.
    pub fn node() -> Self {
        Self {
            our_type: OurType::Node,
            ..Self::default()
        }
    }

    /// Create `Config` for client.
    pub fn client() -> Self {
        Self {
            our_type: OurType::Client,
            ..Self::default()
        }
    }

    /// Set the `hard_coded_contacts`.
    pub fn with_hard_coded_contacts<I>(self, contacts: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<NodeInfo>,
    {
        Self {
            hard_coded_contacts: contacts.into_iter().map(Into::into).collect(),
            ..self
        }
    }

    /// Set the `hard_coded_contacts` to a single contact.
    pub fn with_hard_coded_contact<T>(self, contact: T) -> Self
    where
        T: Into<NodeInfo>,
    {
        self.with_hard_coded_contacts(iter::once(contact))
    }

    /// Set the endpoint (IP + port) to use.
    pub fn with_endpoint(self, addr: SocketAddr) -> Self {
        Self {
            ip: Some(addr.ip()),
            port: Some(addr.port()),
            ..self
        }
    }
}

/// The type of our `QuicP2p` instance: client or node.
#[derive(Clone, Copy, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub enum OurType {
    /// We are a client
    Client,
    /// We are a node
    Node,
}

impl Default for OurType {
    fn default() -> Self {
        Self::Node
    }
}

impl FromStr for OurType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "client" => Ok(Self::Client),
            "node" => Ok(Self::Node),
            x => Err(format!("Unknown client type: {}", x)),
        }
    }
}

/// Events from `QuicP2p` to the user.
#[derive(Debug)]
pub enum Event {
    /// Bootstrap failed.
    BootstrapFailure,
    /// Bootstrap succeeded.
    BootstrappedTo {
        /// Info about the node we are bootstrapped to.
        node: NodeInfo,
    },
    /// Connection to the given address failed.
    ConnectionFailure {
        /// Address of the peer we attempted connecting to.
        peer_addr: SocketAddr,
        /// Error explaining connection failure.
        err: Error,
    },
    /// Message sent by us but not delivered due to connection drop.
    UnsentUserMessage {
        /// Intended message recipient.
        peer_addr: SocketAddr,
        /// Message content.
        msg: Bytes,
        /// Message Token
        token: Token,
    },
    /// Message sent by us and we won't receive UnsentUserMessage for this one.
    /// Either it was sent successfully or it will fail too late for the failure
    /// to be detected.
    /// In most cases, this should be synonymous with success. It is safe to consider
    /// a failure beyond this point as a byzantine fault.
    SentUserMessage {
        /// Intended message recipient.
        peer_addr: SocketAddr,
        /// Message content.
        msg: Bytes,
        /// Message Token
        token: Token,
    },
    /// Connection successfully established.
    ConnectedTo {
        /// Info about the connected peer.
        peer: Peer,
    },
    /// Message received.
    NewMessage {
        /// Message sender.
        peer_addr: SocketAddr,
        /// Message content.
        msg: Bytes,
    },
    /// Sent right before the `QuickP2p` instance drops.
    Finish,
}

/// Information about peer.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Peer {
    /// Peer of type node.
    Node {
        /// Information about the node (it's address and certificate).
        node_info: NodeInfo,
    },
    /// Peer of type client.
    Client {
        /// Address of the client.
        peer_addr: SocketAddr,
    },
}

impl Peer {
    /// Create `Peer` with the given type and address.
    pub fn new(peer_type: OurType, addr: SocketAddr) -> Self {
        match peer_type {
            OurType::Client => Self::Client { peer_addr: addr },
            OurType::Node => Self::Node {
                node_info: NodeInfo::from(addr),
            },
        }
    }

    /// Create `Peer::Node` with the given address.
    pub fn node(addr: SocketAddr) -> Self {
        Self::Node {
            node_info: NodeInfo::from(addr),
        }
    }

    /// Return the peer address.
    pub fn peer_addr(&self) -> SocketAddr {
        match *self {
            Self::Node { ref node_info } => node_info.peer_addr,
            Self::Client { peer_addr } => peer_addr,
        }
    }
}

/// Information about a peer of type node.
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, StructOpt)]
pub struct NodeInfo {
    /// Endpoint of the node
    pub peer_addr: SocketAddr,
    /// Certificate of the node
    pub peer_cert_der: Vec<u8>,
}

impl From<SocketAddr> for NodeInfo {
    fn from(addr: SocketAddr) -> Self {
        Self {
            peer_addr: addr,
            peer_cert_der: vec![],
        }
    }
}
