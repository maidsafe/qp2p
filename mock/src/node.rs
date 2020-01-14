// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::{
    network::{Inner, Packet, NETWORK},
    Config, Error, Event, NodeInfo, OurType, Peer,
};
use bytes::Bytes;
use crossbeam_channel::Sender;
// Note: using `FxHashMap` / `FxHashSet` because they don't use random state and thus guarantee
// consistent iteration order (necessary for repeatable tests). Can't use `BTreeMap` / `BTreeSet`
// because we key by `SocketAddr` which doesn't implement `Ord`.
use fxhash::{FxHashMap, FxHashSet};
use std::{cell::RefCell, net::SocketAddr, rc::Rc};

pub(super) struct Node {
    network: Rc<RefCell<Inner>>,
    addr: SocketAddr,
    event_tx: Sender<Event>,
    config: Config,
    peers: FxHashMap<SocketAddr, ConnectionType>,
    bootstrap_cache: FxHashSet<NodeInfo>,
    pending_bootstraps: FxHashSet<SocketAddr>,
    pending_messages: FxHashMap<SocketAddr, Vec<(Bytes, u64)>>,
}

impl Node {
    pub fn new(event_tx: Sender<Event>, config: Config) -> Rc<RefCell<Self>> {
        let network = NETWORK.with(|network| {
            Rc::clone(
                network
                    .borrow()
                    .as_ref()
                    .expect("Mock Network must exist before creating instances of QuicP2p."),
            )
        });

        let addr = network.borrow_mut().gen_addr(config.ip, config.port);
        let node = Rc::new(RefCell::new(Self {
            network,
            addr,
            event_tx,
            config,
            peers: Default::default(),
            bootstrap_cache: Default::default(),
            pending_bootstraps: Default::default(),
            pending_messages: Default::default(),
        }));
        node.borrow()
            .network
            .borrow_mut()
            .insert_node(addr, Rc::clone(&node));
        node
    }

    pub fn bootstrap(&mut self) {
        if self
            .peers
            .values()
            .cloned()
            .any(ConnectionType::is_bootstrap)
        {
            return;
        }

        if self.config.hard_coded_contacts.is_empty() && self.bootstrap_cache.is_empty() {
            // No one to bootstrap to.
            self.fire_event(Event::BootstrapFailure);
            return;
        }

        for contact in self
            .config
            .hard_coded_contacts
            .iter()
            .chain(&self.bootstrap_cache)
        {
            let _ = self.pending_bootstraps.insert(contact.peer_addr);
            self.network.borrow_mut().send(
                self.addr,
                contact.peer_addr,
                Packet::BootstrapRequest(self.config.our_type),
            )
        }
    }

    pub fn connect(&self, dst: SocketAddr) {
        if self.peers.contains_key(&dst) {
            // Connection already exists
            return;
        }

        self.send_connect_request(dst)
    }

    pub fn disconnect(&mut self, dst: SocketAddr) {
        if self.peers.remove(&dst).is_some() {
            self.network.borrow_mut().disconnect(self.addr, dst)
        }
    }

    pub fn send(&mut self, dst: SocketAddr, msg: Bytes, token: u64) {
        if self.peers.contains_key(&dst) {
            self.send_message(dst, msg, token)
        } else {
            self.send_connect_request(dst);
            self.add_pending_message(dst, msg, token)
        }
    }

    pub fn receive_packet(&mut self, src: SocketAddr, packet: Packet) -> Option<Packet> {
        match packet {
            Packet::BootstrapRequest(peer_type) => {
                if self.peers.insert(src, ConnectionType::Bootstrap).is_none() {
                    self.network
                        .borrow_mut()
                        .send(self.addr, src, Packet::BootstrapSuccess);

                    self.fire_event(Event::ConnectedTo {
                        peer: Peer::new(peer_type, src),
                    })
                }
            }
            Packet::BootstrapSuccess => {
                if !self
                    .peers
                    .values()
                    .cloned()
                    .any(ConnectionType::is_bootstrap)
                {
                    let _ = self.peers.insert(src, ConnectionType::Bootstrap);
                    self.pending_bootstraps.clear();

                    self.fire_event(Event::BootstrappedTo {
                        node: NodeInfo::from(src),
                    })
                } else {
                    self.network
                        .borrow_mut()
                        .send(self.addr, src, Packet::Disconnect)
                }
            }
            Packet::BootstrapFailure => {
                if !self
                    .peers
                    .values()
                    .cloned()
                    .any(ConnectionType::is_bootstrap)
                {
                    let _ = self.pending_bootstraps.remove(&src);

                    if self.pending_bootstraps.is_empty() {
                        self.fire_event(Event::BootstrapFailure)
                    }
                }
            }
            Packet::ConnectRequest(peer_type) => {
                if self.peers.insert(src, ConnectionType::Normal).is_none() {
                    self.network
                        .borrow_mut()
                        .send(self.addr, src, Packet::ConnectSuccess);
                    self.send_pending_messages(src);

                    self.fire_event(Event::ConnectedTo {
                        peer: Peer::new(peer_type, src),
                    })
                }
            }
            Packet::ConnectSuccess => {
                if self.peers.insert(src, ConnectionType::Normal).is_none() {
                    let _ = self.bootstrap_cache.insert(NodeInfo::from(src));
                    self.send_pending_messages(src);

                    self.fire_event(Event::ConnectedTo {
                        peer: Peer::node(src),
                    });
                }
            }
            Packet::ConnectFailure => {
                // Note: the real quic-p2p does not emit anything on unsuccessful connection
                // attempts, only when a previously successfully established connection gets
                // dropped, but it will in the future.
                self.clear_pending_messages(src);
            }
            Packet::Message(msg, token) => {
                if self.peers.contains_key(&src) {
                    self.fire_event(Event::NewMessage {
                        peer_addr: src,
                        msg: msg.clone(),
                    });
                    return Some(Packet::MessageSent(msg, token));
                } else {
                    return Some(Packet::MessageFailure(msg, token));
                }
            }
            Packet::MessageFailure(msg, token) => self.fire_event(Event::UnsentUserMessage {
                peer_addr: src,
                msg,
                token,
            }),
            Packet::MessageSent(msg, token) => self.fire_event(Event::SentUserMessage {
                peer_addr: src,
                msg,
                token,
            }),
            Packet::Disconnect => {
                self.clear_pending_messages(src);
                if self.peers.remove(&src).is_some() {
                    self.fire_event(Event::ConnectionFailure {
                        peer_addr: src,
                        err: Error,
                    })
                }
            }
        }

        None
    }

    pub fn our_connection_info(&self) -> Result<NodeInfo, Error> {
        match self.config.our_type {
            OurType::Client => Err(Error),
            OurType::Node => Ok(NodeInfo::from(self.addr)),
        }
    }

    pub fn bootstrap_cache(&self) -> Vec<NodeInfo> {
        self.bootstrap_cache.iter().cloned().collect()
    }

    pub fn is_connected(&self, addr: &SocketAddr) -> bool {
        self.peers.get(addr).is_some()
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    fn fire_event(&self, event: Event) {
        let _ = self.event_tx.send(event);
    }

    fn send_connect_request(&self, dst: SocketAddr) {
        self.network
            .borrow_mut()
            .send(self.addr, dst, Packet::ConnectRequest(self.config.our_type))
    }

    fn send_message(&self, dst: SocketAddr, msg: Bytes, token: u64) {
        self.network
            .borrow_mut()
            .send(self.addr, dst, Packet::Message(msg, token))
    }

    fn add_pending_message(&mut self, addr: SocketAddr, msg: Bytes, token: u64) {
        self.pending_messages
            .entry(addr)
            .or_insert_with(Default::default)
            .push((msg, token))
    }

    fn send_pending_messages(&mut self, addr: SocketAddr) {
        let messages = if let Some(messages) = self.pending_messages.remove(&addr) {
            messages
        } else {
            return;
        };

        for (msg, token) in messages {
            self.send_message(addr, msg, token)
        }
    }

    fn clear_pending_messages(&mut self, addr: SocketAddr) {
        let messages = if let Some(messages) = self.pending_messages.remove(&addr) {
            messages
        } else {
            return;
        };

        for (msg, token) in messages {
            self.fire_event(Event::UnsentUserMessage {
                peer_addr: addr,
                msg,
                token,
            })
        }
    }
}

#[cfg(test)]
impl Node {
    pub fn addr(&self) -> &SocketAddr {
        &self.addr
    }

    pub fn our_type(&self) -> OurType {
        self.config.our_type
    }
}

impl Drop for Node {
    fn drop(&mut self) {
        for (dst, _) in self.peers.drain() {
            self.network.borrow_mut().disconnect(self.addr, dst)
        }

        self.network.borrow_mut().remove_node(&self.addr);
        self.fire_event(Event::Finish)
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ConnectionType {
    // Connection established via `connect_to`.
    Normal,
    // Connection established via `bootstrap`.
    Bootstrap,
}

impl ConnectionType {
    fn is_bootstrap(self) -> bool {
        match self {
            Self::Normal => false,
            Self::Bootstrap => true,
        }
    }
}
