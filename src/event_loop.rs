// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use log::{debug, warn};
use std::fmt;
use std::thread::{self, JoinHandle};
use tokio::sync::mpsc::{self, UnboundedSender};
use unwrap::unwrap;

/// Post messages to event loop
pub fn post<F>(tx: &mut UnboundedSender<EventLoopMsg>, f: F)
where
    F: FnOnce() + Send + 'static,
{
    let msg = EventLoopMsg::new(f);
    if let Err(e) = tx.send(msg) {
        warn!("Error posting messages to event loop: {:?}", e);
    }
}

/// Message that event loop can accept in order to be requested to do something
pub struct EventLoopMsg(Option<Box<dyn FnMut() + Send>>);

impl EventLoopMsg {
    /// Create a new message to be posted to the event loop
    pub fn new<F>(f: F) -> Self
    where
        F: FnOnce() + Send + 'static,
    {
        let mut f = Some(f);
        EventLoopMsg(Some(Box::new(move || {
            let f = unwrap!(f.take());
            f()
        })))
    }

    /// Create a terminator message which when posted is going to try and exit the event loop. This
    /// is for graceful termination
    pub fn terminator() -> Self {
        Self(None)
    }
}

impl fmt::Debug for EventLoopMsg {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "EventLoopMsg with {} inner",
            if self.0.is_some() { "some" } else { "no" }
        )
    }
}

pub struct EventLoop {
    tx: UnboundedSender<EventLoopMsg>,
    j: Option<JoinHandle<()>>,
}

impl EventLoop {
    pub fn spawn() -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<EventLoopMsg>();

        let j = thread::Builder::new()
            .name("QuicP2p-Event-Loop".into())
            .spawn(move || {
                let event_loop_future = async move {
                    while let Some(ev_loop_msg) = rx.recv().await {
                        if let Some(mut f) = ev_loop_msg.0 {
                            f();
                        } else {
                            break;
                        }
                    }
                };

                let mut runtime = tokio::runtime::Builder::new()
                    .enable_all()
                    .basic_scheduler()
                    .build()
                    .expect("Cannot start Tokio runtime, aborting");
                runtime.block_on(event_loop_future);

                debug!("Exiting QuicP2p Event Loop");
            })
            .expect("Cannot start QuicP2P event loop thread, aborting");

        Self { tx, j: Some(j) }
    }

    #[allow(unused)]
    pub fn tx(&mut self) -> &mut UnboundedSender<EventLoopMsg> {
        &mut self.tx
    }

    /// Post messages to event loop
    pub fn post<F>(&mut self, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        post(&mut self.tx, f)
    }
}

impl Drop for EventLoop {
    fn drop(&mut self) {
        if let Err(e) = self.tx.send(EventLoopMsg::terminator()) {
            warn!("Error trying to send an event loop terminator: {:?}", e);
        }
        let j = unwrap!(self.j.take());
        if let Err(e) = j.join() {
            warn!("Error joining the event loop thread: {:?}", e);
        }
    }
}
