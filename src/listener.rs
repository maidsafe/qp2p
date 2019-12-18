// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::communicate;
use crate::connection::{BootstrapGroupRef, Connection, FromPeer, QConn, ToPeer};
use crate::context::ctx_mut;
use crate::event::Event;
use crate::utils;
use crate::Error;
use crate::NodeInfo;
use futures::future::{self, TryFutureExt};
use futures::stream::StreamExt;
use log::{debug, info, warn};

/// Start listening
pub fn listen(incoming_connections: quinn::Incoming) {
    let leaf = incoming_connections.for_each(move |connecting| {
        let leaf = connecting
            .map_err(|e| debug!("New connection errored out: {}", e))
            .map_ok(|new_connection| handle_new_conn(new_connection));
        let _ = tokio::spawn(leaf);
        future::ready(())
    });

    let _ = tokio::spawn(leaf);
}

enum Action {
    HandleDuplicate(QConn),
    HandleAlreadyBootstrapped,
    Continue(Option<BootstrapGroupRef>),
}

fn handle_new_conn(
    quinn::NewConnection {
        driver,
        connection,
        uni_streams,
        bi_streams,
        ..
    }: quinn::NewConnection,
) {
    let q_conn = QConn::from(connection);

    let peer_addr = q_conn.remote_address();

    let _ = tokio::spawn(driver.map_err(move |e| {
        utils::handle_communication_err(peer_addr, &From::from(e), "Driver failed", None);
    }));

    let state = ctx_mut(|c| {
        let event_tx = c.event_tx.clone();
        let conn = c
            .connections
            .entry(peer_addr)
            .or_insert_with(|| Connection::new(peer_addr, event_tx, None));
        if conn.from_peer.is_no_connection() {
            conn.from_peer = FromPeer::Established {
                q_conn,
                pending_reads: Default::default(),
            };

            let bootstrap_group = if let ToPeer::Established {
                ref peer_cert_der, ..
            } = conn.to_peer
            {
                let node_info = NodeInfo {
                    peer_addr,
                    peer_cert_der: peer_cert_der.clone(),
                };
                let mut bootstrap_group = None;

                // TODO come back to all the connected-to events and see if we are handling all
                // cases
                let event = if let Some(bootstrap_group_ref) = conn.bootstrap_group_ref.take() {
                    if bootstrap_group_ref.is_bootstrap_successful_yet() {
                        return Action::HandleAlreadyBootstrapped;
                    }
                    bootstrap_group = Some(bootstrap_group_ref);
                    Event::BootstrappedTo { node: node_info }
                } else {
                    Event::ConnectedTo {
                        peer: node_info.into(),
                    }
                };

                if let Err(e) = c.event_tx.send(event) {
                    info!("ERROR in informing user about a new peer: {:?} - {}", e, e);
                }

                bootstrap_group
            } else {
                None
            };
            Action::Continue(bootstrap_group)
        } else {
            Action::HandleDuplicate(q_conn)
        }
    });

    match state {
        Action::HandleDuplicate(_q_conn) => {
            debug!("Not allowing duplicate connection from peer: {}", peer_addr);
        }
        Action::HandleAlreadyBootstrapped => crate::utils::handle_communication_err(
            peer_addr,
            &Error::ConnectionCancelled,
            "Connection already bootstrapped",
            None,
        ),
        Action::Continue(bootstrap_group) => {
            if let Some(bootstrap_group_ref) = bootstrap_group {
                bootstrap_group_ref.terminate_group(true);
            }
            communicate::read_from_peer(peer_addr, uni_streams, bi_streams);
        }
    }
}
