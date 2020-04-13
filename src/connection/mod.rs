// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

pub use self::bootstrap_group::{BootstrapGroupMaker, BootstrapGroupRef};
pub use self::from_peer::FromPeer;
pub use self::q_conn::QConn;
pub use self::to_peer::ToPeer;

use crate::{context::ctx_mut, error::QuicP2pError, event::Event, peer::Peer, EventSenders};
use futures::future::FutureExt;
use log::trace;
use std::{collections::hash_map::Entry, fmt, net::SocketAddr, time::Duration};

mod bootstrap_group;
mod from_peer;
mod q_conn;
mod to_peer;

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
    event_tx: EventSenders,
}

impl Connection {
    /// New Connection with defaults
    pub fn new(
        peer_addr: SocketAddr,
        event_tx: EventSenders,
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
        if self.we_contacted_peer
            || ((self.to_peer.is_established() || self.to_peer.is_not_needed())
                && (self.from_peer.is_established() || self.from_peer.is_not_needed()))
        {
            let peer = match self.to_peer {
                ToPeer::Initiated { .. } | ToPeer::Established { .. } => Peer::Node(self.peer_addr),
                ToPeer::NoConnection | ToPeer::NotNeeded => Peer::Client(self.peer_addr),
            };

            // No need to log these as this will fire even when the QuicP2p handle is dropped and at
            // that point there might be no one listening so sender will error out
            let _ = self.event_tx.send(Event::ConnectionFailure {
                peer,
                err: QuicP2pError::ConnectionCancelled,
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

fn spawn_incomplete_conn_killer(peer_addr: SocketAddr) {
    let leaf =
        tokio::time::delay_for(Duration::from_secs(KILL_INCOMPLETE_CONN_SEC)).map(move |()| {
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
                    let _ = conn.remove();
                }
            });
        });

    // TODO find a way to cancel this timer if we know the connection is done. Otherwise it
    // might delay a clean exit of event loop if we were to use tokio::run() instead
    // of block_on as just now in event_loop.rs
    let _ = tokio::spawn(leaf);
}
