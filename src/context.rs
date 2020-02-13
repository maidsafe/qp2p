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
use crate::connection::Connection;
use crate::EventSenders;
use std::cell::RefCell;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::mpsc;

thread_local! {
    pub static CTX: RefCell<Option<Context>> = RefCell::new(None);
}

/// Initialise `Context`. This will panic if the context has already been initialised for the
/// current thread context.
pub fn initialise_ctx(context: Context) {
    CTX.with(|ctx_refcell| {
        let mut ctx = ctx_refcell.borrow_mut();
        if ctx.is_some() {
            panic!("Context already initialised!");
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

/// Obtain a reference to the `Context`. This will panic if the `Context` has not been set in the
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
            panic!("Context not initialised!");
        }
    })
}

/// Obtain a mutable reference to the `Context`. This will panic if the `Context` has not been set in
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
            panic!("Context not initialised!");
        }
    })
}

/// The context to the event loop. This holds all the states that are necessary to be persistant
/// between calls to poll the event loop for the next event.
pub struct Context {
    pub event_tx: EventSenders,
    pub connections: HashMap<SocketAddr, Connection>,
    pub our_ext_addr_tx: Option<mpsc::Sender<SocketAddr>>,
    pub our_complete_cert: SerialisableCertificate,
    pub max_msg_size_allowed: usize,
    pub our_type: OurType,
    pub bootstrap_cache: BootstrapCache,
    pub(crate) quic_ep: quinn::Endpoint,
    pub(crate) quic_client_cfg: quinn::ClientConfig,
}

impl Context {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        event_tx: EventSenders,
        our_complete_cert: SerialisableCertificate,
        max_msg_size_allowed: usize,
        our_type: OurType,
        bootstrap_cache: BootstrapCache,
        quic_ep: quinn::Endpoint,
        quic_client_cfg: quinn::ClientConfig,
    ) -> Self {
        Self {
            event_tx,
            connections: Default::default(),
            our_ext_addr_tx: Default::default(),
            our_complete_cert,
            max_msg_size_allowed,
            our_type,
            bootstrap_cache,
            quic_ep,
            quic_client_cfg,
        }
    }

    #[cfg(not(test))]
    pub fn quic_ep(&self) -> &quinn::Endpoint {
        &self.quic_ep
    }
}
