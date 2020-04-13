// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crossbeam_channel as mpmc;
use quic_p2p::{Event, EventSenders, Peer};
use serde::{Deserialize, Serialize};

/// Remote procedure call for our examples to communicate.
#[derive(Debug, Serialize, Deserialize)]
pub enum Rpc {
    /// Starts the connectivity and data exchange test between us and given QuicP2p peers.
    StartTest(Vec<Peer>),
}

/// Receivers side for events
pub struct EventReceivers {
    pub node_rx: mpmc::Receiver<Event>,
    pub client_rx: mpmc::Receiver<Event>,
}

impl EventReceivers {
    #[allow(unused)]
    pub fn recv(&self) -> Result<Event, mpmc::RecvError> {
        let mut sel = mpmc::Select::new();
        let client_idx = sel.recv(&self.client_rx);
        let node_idx = sel.recv(&self.node_rx);
        let selected_operation = sel.ready();

        if selected_operation == client_idx {
            self.client_rx.recv()
        } else if selected_operation == node_idx {
            self.node_rx.recv()
        } else {
            panic!("invalid operation");
        }
    }

    #[allow(unused)]
    pub fn iter(&self) -> IterEvent {
        IterEvent { event_rx: &self }
    }
}

pub struct IterEvent<'a> {
    event_rx: &'a EventReceivers,
}

impl<'a> Iterator for IterEvent<'a> {
    type Item = Event;

    fn next(&mut self) -> Option<Self::Item> {
        let mut sel = mpmc::Select::new();
        let client_idx = sel.recv(&self.event_rx.client_rx);
        let node_idx = sel.recv(&self.event_rx.node_rx);
        let selected_operation = sel.ready();

        let event = if selected_operation == client_idx {
            self.event_rx.client_rx.recv()
        } else if selected_operation == node_idx {
            self.event_rx.node_rx.recv()
        } else {
            return None;
        };

        event.ok()
    }
}

/// Create channels for events
pub fn new_unbounded_channels() -> (EventSenders, EventReceivers) {
    let (client_tx, client_rx) = mpmc::unbounded();
    let (node_tx, node_rx) = mpmc::unbounded();
    (
        EventSenders { node_tx, client_tx },
        EventReceivers { node_rx, client_rx },
    )
}
