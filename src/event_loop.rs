use crate::event::Event;
use futures::{Future, Stream};
use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;
use std::sync::mpsc::Sender;
use std::thread::{self, JoinHandle};
use tokio::runtime::current_thread;
use tokio::sync::mpsc::{self, UnboundedSender};

/// Message that event loop can accept in order to be requested to do something
pub struct EventLoopMsg(Option<Box<FnMut(&EventLoopState) + Send>>);

impl EventLoopMsg {
    /// Create a new message to be posted to the event loop
    pub fn new<F>(f: F) -> Self
    where
        F: FnOnce(&EventLoopState) + Send + 'static,
    {
        let mut f = Some(f);
        EventLoopMsg(Some(Box::new(move |el_state| {
            let f = unwrap!(f.take());
            f(el_state)
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

#[derive(Clone)]
pub struct EventLoopState {
    inner: Rc<RefCell<Inner>>,
}

struct Inner {
    event_tx: Sender<Event>,
    quic_ep: Option<quinn::Endpoint>,
}

impl EventLoopState {
    /// Insert a QUIC Endpoint if not already. If already done previously this will return false.
    pub fn insert_quic_endpoint(&self, quic_ep: quinn::Endpoint) -> bool {
        let mut inner = self.inner.borrow_mut();
        if inner.quic_ep.is_none() {
            inner.quic_ep = Some(quic_ep);
            true
        } else {
            false
        }
    }

    /// Crust event sender to users
    pub fn tx(&self) -> Sender<Event> {
        self.inner.borrow_mut().event_tx.clone()
    }

    fn new(event_tx: Sender<Event>) -> Self {
        Self {
            inner: Rc::new(RefCell::new(Inner {
                event_tx,
                quic_ep: None,
            })),
        }
    }
}

pub struct EventLoop {
    tx: UnboundedSender<EventLoopMsg>,
    j: Option<JoinHandle<()>>,
}

impl EventLoop {
    pub fn spawn(event_tx: Sender<Event>) -> Self {
        let (tx, rx) = mpsc::unbounded_channel::<EventLoopMsg>();

        let j = unwrap!(thread::Builder::new()
            .name("Crust-Event-Loop".into())
            .spawn(move || {
                let el_state = EventLoopState::new(event_tx);
                let event_loop_future = rx.map_err(|_| ()).for_each(move |ev_loop_msg| {
                    if let Some(mut f) = ev_loop_msg.0 {
                        f(&el_state);
                        Ok(())
                    } else {
                        Err(())
                    }
                });

                current_thread::run(event_loop_future);
                println!("Exiting Crust Event Loop");
            }));

        Self { tx, j: Some(j) }
    }

    pub fn tx(&mut self) -> &mut UnboundedSender<EventLoopMsg> {
        &mut self.tx
    }

    /// Post messages to event loop
    pub fn post<F>(tx: &mut UnboundedSender<EventLoopMsg>, f: F)
    where
        F: FnOnce(&EventLoopState) + Send + 'static,
    {
        let msg = EventLoopMsg::new(f);
        if let Err(e) = tx.try_send(msg) {
            println!("Error posting messages to event loop: {:?}", e);
        }
    }
}

impl Drop for EventLoop {
    fn drop(&mut self) {
        if let Err(e) = self.tx.try_send(EventLoopMsg::terminator()) {
            println!("Error trying to send an event loop terminator: {:?}", e);
        }
        let j = unwrap!(self.j.take());
        if let Err(e) = j.join() {
            println!("Error joining the event loop thread: {:?}", e);
        }
    }
}
