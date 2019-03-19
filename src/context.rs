use crate::config::SerialisableCeritificate;
use crate::event::Event;
use crate::wire_msg::WireMsg;
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::net::SocketAddr;
use std::sync::mpsc::Sender;

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
    pub our_complete_cert: SerialisableCeritificate,
    pub max_msg_size_allowed: usize,
    quic_ep: quinn::Endpoint,
}

impl Context {
    pub fn new(
        event_tx: Sender<Event>,
        our_complete_cert: SerialisableCeritificate,
        max_msg_size_allowed: usize,
        quic_ep: quinn::Endpoint,
    ) -> Self {
        Self {
            event_tx,
            connections: Default::default(),
            our_ext_addr_tx: Default::default(),
            our_complete_cert,
            max_msg_size_allowed,
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
    peer_addr: SocketAddr,
    event_tx: Sender<Event>,
}

impl Connection {
    pub fn new(peer_addr: SocketAddr, event_tx: Sender<Event>) -> Self {
        Self {
            to_peer: Default::default(),
            from_peer: Default::default(),
            peer_addr,
            event_tx,
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        // TODO Improve this further by noting down if we intitated the connection OR if we fired
        // connect success to the user. Onle in these cases fire the failure.
        if !self.to_peer.is_no_connection() {
            if let Err(e) = self.event_tx.send(Event::ConnectionFailure {
                peer_addr: self.peer_addr,
            }) {
                // TODO log this as trace as this will happen frequently when the user drops itself
                println!("Error informing ConnectionFailure: {:?}", e);
            }
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
    Initiated {
        peer_cert_der: Vec<u8>,
        pending_sends: VecDeque<WireMsg>,
    },
    Established {
        peer_cert_der: Vec<u8>,
        q_conn: quinn::Connection,
    },
}

impl ToPeer {
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

    #[allow(unused)]
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
            ToPeer::NoConnection => write!(f, "ToPeer::NoConnection",),
            ToPeer::Initiated {
                ref pending_sends, ..
            } => write!(
                f,
                "ToPeer::Initiated with {} pending sends",
                pending_sends.len()
            ),
            ToPeer::Established { .. } => write!(f, "ToPeer::Established"),
        }
    }
}

pub enum FromPeer {
    NoConnection,
    Established {
        q_conn: quinn::Connection,
        pending_reads: VecDeque<WireMsg>,
    },
}

impl FromPeer {
    pub fn is_no_connection(&self) -> bool {
        if let FromPeer::NoConnection = *self {
            true
        } else {
            false
        }
    }

    #[allow(unused)]
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
            FromPeer::NoConnection => write!(f, "FromPeer::NoConnection"),
            FromPeer::Established {
                ref pending_reads, ..
            } => write!(
                f,
                "FromPeer::Established with {} pending reads",
                pending_reads.len()
            ),
        }
    }
}
