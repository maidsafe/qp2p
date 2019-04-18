// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::bootstrap_cache::BootstrapCache;
use crate::config::{OurType, SerialisableCertificate};
use crate::event::Event;
use crate::wire_msg::WireMsg;
use std::cell::RefCell;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::fmt;
use std::net::SocketAddr;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};
use tokio::prelude::Future;
use tokio::runtime::current_thread;
use tokio::timer::Delay;

pub type Terminator = tokio::sync::mpsc::Sender<()>;

const KILL_INCOMPLETE_CONN_SEC: u64 = 60;

thread_local! {
    pub static CTX: RefCell<Option<Context>> = RefCell::new(None);
}

/// Initialise `Context`. This will panic if the context has already been initialised for the
/// current thread context.
pub fn initialise_ctx(context: Context) {
    CTX.with(|ctx_refcell| {
        let mut ctx = ctx_refcell.borrow_mut();
        if ctx.is_some() {
            panic!("Context already initialised !");
        } else {
            *ctx = Some(context);
        }
    })
}

/// Check if the `Context` is already initialised for the current thread context.
#[allow(unused)]
pub fn is_ctx_initialised() -> bool {
    CTX.with(|ctx_refcell| {
        let ctx = ctx_refcell.borrow();
        ctx.is_some()
    })
}

/// Obtain a referece to the `Context`. This will panic if the `Context` has not be set in the
/// current thread context.
pub fn ctx<F, R>(f: F) -> R
where
    F: FnOnce(&Context) -> R,
{
    CTX.with(|ctx_refcell| {
        let ctx = ctx_refcell.borrow();
        if let Some(ctx) = ctx.as_ref() {
            f(ctx)
        } else {
            panic!("Context not initialised !");
        }
    })
}

/// Obtain a mutable referece to the `Context`. This will panic if the `Context` has not be set in
/// the current thread context.
pub fn ctx_mut<F, R>(f: F) -> R
where
    F: FnOnce(&mut Context) -> R,
{
    CTX.with(|ctx_refcell| {
        let mut ctx = ctx_refcell.borrow_mut();
        if let Some(ctx) = ctx.as_mut() {
            f(ctx)
        } else {
            panic!("Context not initialised !");
        }
    })
}

/// The context to the event loop. This holds all the states that are necessary to be persistant
/// between calls to poll the event loop for the next event.
pub struct Context {
    pub event_tx: Sender<Event>,
    pub connections: HashMap<SocketAddr, Connection>,
    pub our_ext_addr_tx: Option<Sender<SocketAddr>>,
    pub our_complete_cert: SerialisableCertificate,
    pub max_msg_size_allowed: usize,
    pub idle_timeout_msec: u64,
    pub keep_alive_interval_msec: u32,
    pub our_type: OurType,
    pub bootstrap_cache: BootstrapCache,
    quic_ep: quinn::Endpoint,
}

impl Context {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        event_tx: Sender<Event>,
        our_complete_cert: SerialisableCertificate,
        max_msg_size_allowed: usize,
        idle_timeout_msec: u64,
        keep_alive_interval_msec: u32,
        our_type: OurType,
        bootstrap_cache: BootstrapCache,
        quic_ep: quinn::Endpoint,
    ) -> Self {
        Self {
            event_tx,
            connections: Default::default(),
            our_ext_addr_tx: Default::default(),
            our_complete_cert,
            max_msg_size_allowed,
            idle_timeout_msec,
            keep_alive_interval_msec,
            our_type,
            bootstrap_cache,
            quic_ep,
        }
    }

    pub fn quic_ep(&self) -> &quinn::Endpoint {
        &self.quic_ep
    }
}

pub struct Connection {
    pub to_peer: ToPeer,
    pub from_peer: FromPeer,

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
    pub fn new(peer_addr: SocketAddr, event_tx: Sender<Event>) -> Self {
        spawn_incomplete_conn_killer(peer_addr);

        Self {
            to_peer: Default::default(),
            from_peer: Default::default(),
            peer_addr,
            event_tx,
            we_contacted_peer: Default::default(),
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        if (self.to_peer.is_established() || self.to_peer.is_not_needed())
            && (self.from_peer.is_established() || self.from_peer.is_not_needed())
        {
            // No need to log this as this will fire even when the QuicP2p handle is dropped and at
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
        q_conn: quinn::Connection,
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
            ToPeer::NotNeeded | ToPeer::NoConnection => {}
            ToPeer::Initiated {
                ref mut terminator, ..
            } => {
                let _ = terminator.try_send(());
            }
            ToPeer::Established { ref q_conn, .. } => q_conn.clone().close(0, &[]),
        }
    }
}

pub enum FromPeer {
    NoConnection,
    NotNeeded,
    Established {
        q_conn: quinn::Connection,
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

impl Drop for FromPeer {
    fn drop(&mut self) {
        if let FromPeer::Established { ref q_conn, .. } = *self {
            q_conn.clone().close(0, &[]);
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
