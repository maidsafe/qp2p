// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

//! A `BootstrapGroup` is a group of related connection attempts that is spawned as a result of a
//! bootstrap attempt off the list of bootstrap nodes available to us. This group can be terminated when
//! any member of the group has been successful in forging a bootstrap connection off a bootstrap node. The
//! termination of the group would result in immediate cancellation of rest of the attempts
//! currently being made by the members of the group and thus an eventual destruction of all such
//! members to not continue to use resources as we no longer require them.

use crate::context::ctx_mut;
use crate::event::Event;
use crate::utils::ConnectTerminator;
use crate::EventSenders;
use log::info;
use std::cell::RefCell;
use std::collections::HashMap;
use std::mem;
use std::net::SocketAddr;
use std::rc::Rc;

/// Creator of a `BootstrapGroup`. Use this to obtain the reference to the undelying group.
///
/// Destroy the maker once all references of the group have been obtained to not hold the internal
/// references for longer than needed. The maker going out of scope is enough for it's destruction.
pub struct BootstrapGroupMaker {
    group: Rc<RefCell<BootstrapGroup>>,
}

impl BootstrapGroupMaker {
    /// Create a handle that refers to a newly created underlying group.
    pub fn new(event_tx: EventSenders) -> Self {
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
        terminator: ConnectTerminator,
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
        let mut terminators = {
            let mut group = self.group.borrow_mut();
            if is_due_to_success {
                group.is_bootstrap_successful_yet = true;
            }

            // We use a `mem::replace` here because `self.group` can be mutably borrowed
            // twice during `BootstrapGroupRef::drop` (it's called when we remove a connection)
            mem::take(&mut group.terminators)
        };

        let _ = terminators.remove(&self.peer_addr);

        ctx_mut(|c| {
            for (peer_addr, mut terminator) in terminators.drain() {
                let _ = terminator.try_send(());
                let _conn = c.connections.remove(&peer_addr);
            }
        });
    }

    pub fn is_bootstrap_successful_yet(&self) -> bool {
        self.group.borrow().is_bootstrap_successful_yet
    }
}

impl Drop for BootstrapGroupRef {
    fn drop(&mut self) {
        let _ = self.group.borrow_mut().terminators.remove(&self.peer_addr);
    }
}

struct BootstrapGroup {
    is_bootstrap_successful_yet: bool,
    terminators: HashMap<SocketAddr, ConnectTerminator>,
    event_tx: EventSenders,
}

impl Drop for BootstrapGroup {
    fn drop(&mut self) {
        if !self.is_bootstrap_successful_yet {
            if let Err(e) = self.event_tx.node_tx.send(Event::BootstrapFailure) {
                info!("Failed informing about bootstrap failure: {:?}", e);
            }
        }
    }
}
