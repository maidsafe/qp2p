// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::context::ctx_mut;
use crate::event::Event;
use crate::utils::QConn;
use crate::wire_msg::WireMsg;
use std::cell::RefCell;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::fmt;
use std::net::SocketAddr;
use std::rc::Rc;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};
use tokio::prelude::Future;
use tokio::runtime::current_thread;
use tokio::timer::Delay;

/// This is to terminate the connection attempt should it take too long to mature to completeness.
pub type Terminator = tokio::sync::mpsc::Sender<()>;

const KILL_INCOMPLETE_CONN_SEC: u64 = 60;

/// Represents a connection to the peer. Depending on the types of peers involved (node or client)
/// the connection might represent a couple of connections internally (to and from peer) or a
/// single connection.
pub struct Connection {
    /// Connection to the peer from us
    pub to_peer: ToPeer,
    /// Connection from the peer to us
    pub from_peer: FromPeer,
    /// If this connection belongs to a bootstap group of connection attempts
    pub bootstrap_group_ref: Option<BootstrapGroupRef>,
    /// quic-p2p won't validate incoming peers, it will simply pass them to the upper layer.
    /// Until we know that these peers are useful/valid for the upper layers, we might refrain
    /// ourselves from taking specific actions: e.g. putting these peers into the bootstrap cache.
    /// This flag indicates whether upper layer attempted to connect/send something to the other
    /// end of this connection.
    pub we_contacted_peer: bool,
    peer_addr: SocketAddr,
    event_tx: Sender<Event>,
}

impl Connection {
    /// New Connection with defaults
    pub fn new(
        peer_addr: SocketAddr,
        event_tx: Sender<Event>,
        bootstrap_group_ref: Option<BootstrapGroupRef>,
    ) -> Self {
        spawn_incomplete_conn_killer(peer_addr);

        Self {
            to_peer: Default::default(),
            from_peer: Default::default(),
            bootstrap_group_ref,
            we_contacted_peer: false,
            peer_addr,
            event_tx,
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        if (self.to_peer.is_established() || self.to_peer.is_not_needed())
            && (self.from_peer.is_established() || self.from_peer.is_not_needed())
        {
            // No need to log these as this will fire even when the QuicP2p handle is dropped and at
            // that point there might be no one listening so sender will error out
            let _ = self.event_tx.send(Event::ConnectionFailure {
                peer_addr: self.peer_addr,
            });
        }
    }
}

impl fmt::Debug for Connection {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Connection {{ to_peer: {:?}, from_peer: {:?} }}",
            self.to_peer, self.from_peer
        )
    }
}

pub enum ToPeer {
    NoConnection,
    NotNeeded,
    Initiated {
        terminator: Terminator,
        peer_cert_der: Vec<u8>,
        pending_sends: Vec<WireMsg>,
    },
    Established {
        peer_cert_der: Vec<u8>,
        q_conn: QConn,
    },
}

impl ToPeer {
    pub fn is_not_needed(&self) -> bool {
        if let ToPeer::NotNeeded = *self {
            true
        } else {
            false
        }
    }

    pub fn is_no_connection(&self) -> bool {
        if let ToPeer::NoConnection = *self {
            true
        } else {
            false
        }
    }

    #[allow(unused)]
    pub fn is_initiated(&self) -> bool {
        if let ToPeer::Initiated { .. } = *self {
            true
        } else {
            false
        }
    }

    pub fn is_established(&self) -> bool {
        if let ToPeer::Established { .. } = *self {
            true
        } else {
            false
        }
    }
}

impl Default for ToPeer {
    fn default() -> Self {
        ToPeer::NoConnection
    }
}

impl fmt::Debug for ToPeer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            ToPeer::Initiated {
                ref pending_sends, ..
            } => write!(
                f,
                "ToPeer::Initiated with {} pending sends",
                pending_sends.len()
            ),
            ToPeer::Established { .. } => write!(f, "ToPeer::Established"),
            ref blah => write!(f, "{:?}", blah),
        }
    }
}

impl Drop for ToPeer {
    fn drop(&mut self) {
        match *self {
            ToPeer::NotNeeded | ToPeer::NoConnection | ToPeer::Established { .. } => {}
            ToPeer::Initiated {
                ref mut terminator, ..
            } => {
                let _ = terminator.try_send(());
            }
        }
    }
}

pub enum FromPeer {
    NoConnection,
    NotNeeded,
    Established {
        q_conn: QConn,
        pending_reads: Vec<WireMsg>,
    },
}

impl FromPeer {
    pub fn is_not_needed(&self) -> bool {
        if let FromPeer::NotNeeded = *self {
            true
        } else {
            false
        }
    }

    pub fn is_no_connection(&self) -> bool {
        if let FromPeer::NoConnection = *self {
            true
        } else {
            false
        }
    }

    pub fn is_established(&self) -> bool {
        if let FromPeer::Established { .. } = *self {
            true
        } else {
            false
        }
    }
}

impl Default for FromPeer {
    fn default() -> Self {
        FromPeer::NoConnection
    }
}

impl fmt::Debug for FromPeer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            FromPeer::Established {
                ref pending_reads, ..
            } => write!(
                f,
                "FromPeer::Established with {} pending reads",
                pending_reads.len()
            ),
            ref blah => write!(f, "{:?}", blah),
        }
    }
}

/// Creator of a `BootstrapGroup`. Use this to obtain the reference to the undelying group.
///
/// Destroy the maker once all references of the group have been obtained to not hold the internal
/// references for longer than needed. The maker going out of scope is enough for it's destruction.
pub struct BootstrapGroupMaker {
    group: Rc<RefCell<BootstrapGroup>>,
}

impl BootstrapGroupMaker {
    /// Create a handle that refers to a newly created underlying group.
    pub fn new(event_tx: Sender<Event>) -> Self {
        Self {
            group: Rc::new(RefCell::new(BootstrapGroup {
                is_bootstrap_successful_yet: false,
                // TODO remove magic number
                terminators: HashMap::with_capacity(300),
                event_tx,
            })),
        }
    }

    /// Add member to the underlying `BootstrapGroup` and get a reference to it.
    pub fn add_member_and_get_group_ref(
        &self,
        peer_addr: SocketAddr,
        terminator: Terminator,
    ) -> BootstrapGroupRef {
        if let Some(mut terminator) = self
            .group
            .borrow_mut()
            .terminators
            .insert(peer_addr, terminator)
        {
            let _ = terminator.try_send(());
        }

        BootstrapGroupRef {
            peer_addr,
            group: self.group.clone(),
        }
    }
}

/// Reference to the underlying `BootstrapGroup`.
///
/// Once all references are dropped (and the `BootstrapGroupMaker` was also dropped) the
/// underlying group will also be destroyed. If the bootstrap was not yet successful by the time
/// this happened, `BootstrapFailure` event will be fired.
pub struct BootstrapGroupRef {
    peer_addr: SocketAddr,
    group: Rc<RefCell<BootstrapGroup>>,
}

impl BootstrapGroupRef {
    /// Prematurely terminate all members of the underlying `BootstrapGroup`. Also indicate if this
    /// is because the bootstrapping was successful (in which case no failure event will be
    /// auto-fired).
    pub fn terminate_group(&self, is_due_to_success: bool) {
        let mut group = self.group.borrow_mut();

        if is_due_to_success {
            group.is_bootstrap_successful_yet = true;
        }

        for (_, mut terminator) in group.terminators.drain() {
            let _ = terminator.try_send(());
        }
    }
}

impl Drop for BootstrapGroupRef {
    fn drop(&mut self) {
        let _ = self.group.borrow_mut().terminators.remove(&self.peer_addr);
    }
}

struct BootstrapGroup {
    is_bootstrap_successful_yet: bool,
    terminators: HashMap<SocketAddr, Terminator>,
    event_tx: Sender<Event>,
}

impl Drop for BootstrapGroup {
    fn drop(&mut self) {
        if !self.is_bootstrap_successful_yet {
            if let Err(e) = self.event_tx.send(Event::BootstrapFailure) {
                info!("Failed informing about bootstrap failure: {:?}", e);
            }
        }
    }
}

fn spawn_incomplete_conn_killer(peer_addr: SocketAddr) {
    let leaf =
        Delay::new(Instant::now() + Duration::from_secs(KILL_INCOMPLETE_CONN_SEC)).then(move |r| {
            if let Err(e) = r {
                info!("Error in incomplete connection killer delay: {:?}", e);
            }

            ctx_mut(|c| {
                let conn = if let Entry::Occupied(oe) = c.connections.entry(peer_addr) {
                    oe
                } else {
                    return;
                };

                if (!conn.get().to_peer.is_established() && !conn.get().to_peer.is_not_needed())
                    || (!conn.get().from_peer.is_established()
                        && !conn.get().from_peer.is_not_needed())
                    || (conn.get().to_peer.is_not_needed() && conn.get().from_peer.is_not_needed())
                {
                    trace!(
                        "Killing a non-completing connection for peer: {}",
                        peer_addr
                    );
                    conn.remove();
                }
            });

            Ok(())
        });

    // TODO find a way to cancel this timer if we know the connection is done. Otherwise it
    // might delay a clean exit of event loop if we were to use current_thread::run() instead
    // of block_on as just now in event_loop.rs
    current_thread::spawn(leaf);
}
