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

pub struct Context {
    pub event_tx: Sender<Event>,
    pub connections: HashMap<SocketAddr, Connection>,
    pub our_ext_addr_tx: Option<Sender<SocketAddr>>,
    quic_ep: quinn::Endpoint,
}

impl Context {
    pub fn new(event_tx: Sender<Event>, quic_ep: quinn::Endpoint) -> Self {
        Self {
            event_tx,
            connections: Default::default(),
            our_ext_addr_tx: Default::default(),
            quic_ep,
        }
    }

    pub fn quic_ep(&self) -> &quinn::Endpoint {
        &self.quic_ep
    }
}

#[derive(Debug, Default)]
pub struct Connection {
    pub to_peer: ToPeer,
    pub from_peer: ConnectionStatus,
}

#[derive(Debug, Default)]
pub struct ToPeer {
    pub status: ConnectionStatus,
    pub pending_sends: VecDeque<WireMsg>,
}

pub enum ConnectionStatus {
    NoConnection,
    Initiated,
    Established(quinn::Connection),
}

impl ConnectionStatus {
    pub fn is_no_connection(&self) -> bool {
        if let ConnectionStatus::NoConnection = *self {
            true
        } else {
            false
        }
    }

    pub fn is_initiated(&self) -> bool {
        if let ConnectionStatus::Initiated = *self {
            true
        } else {
            false
        }
    }

    #[allow(unused)]
    pub fn is_established(&self) -> bool {
        if let ConnectionStatus::Established(_) = *self {
            true
        } else {
            false
        }
    }
}

impl Default for ConnectionStatus {
    fn default() -> Self {
        ConnectionStatus::NoConnection
    }
}

impl fmt::Debug for ConnectionStatus {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            ConnectionStatus::NoConnection => write!(f, "ConnectionStatus::NoConnection"),
            ConnectionStatus::Initiated => write!(f, "ConnectionStatus::Initiated"),
            ConnectionStatus::Established(_) => write!(f, "ConnectionStatus::Established"),
        }
    }
}
