// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::QuicP2p;
use crate::{
    connection::{BootstrapGroupRef, Connection, FromPeer, QConn, ToPeer},
    event::Event,
    peer::Peer,
    QuicP2pError,
};
use futures::future::{self, TryFutureExt};
use futures::stream::StreamExt;
use log::{debug, info, trace};

enum Action {
    HandleDuplicate(QConn),
    HandleAlreadyBootstrapped,
    Continue(Option<BootstrapGroupRef>),
}

impl QuicP2p {
    /// Start listening
    pub fn listen(&self, incoming_connections: quinn::Incoming) {
        let leaf = incoming_connections.for_each(move |connecting| {
            let leaf = connecting
                .map_err(|e| debug!("New connection errored out: {}", e))
                .map_ok(| conn | self.handle_new_incoming_conn(conn));
            let _ = tokio::spawn(leaf);
            future::ready(())
        });

        let _ = tokio::spawn(leaf);
    }

    async fn handle_new_incoming_conn(
        &self,
        quinn::NewConnection {
            connection,
            uni_streams,
            bi_streams,
            ..
        }: quinn::NewConnection,
    ) {
        let q_conn = QConn::from(connection);

        let peer_addr = q_conn.remote_address();

        trace!("Incoming connection from peer: {}", peer_addr);

        let state = {
            let event_tx = self.event_tx.clone();
            let conn = self
                .connections
                .entry(peer_addr)
                .or_insert_with(|| Connection::new(peer_addr, event_tx, None, self.connections));
            if conn.from_peer.is_no_connection() {
                conn.from_peer = FromPeer::Established {
                    q_conn,
                    pending_reads: Default::default(),
                };

                if let ToPeer::Established { .. } = conn.to_peer {
                    // TODO come back to all the connected-to events and see if we are handling all
                    // cases
                    if let Some(mut bootstrap_group_ref) = conn.bootstrap_group_ref.take() {
                        if bootstrap_group_ref.is_bootstrap_successful_yet() {
                            return Action::HandleAlreadyBootstrapped;
                        }

                        if let Err(e) = bootstrap_group_ref
                            .send(Event::BootstrappedTo { node: peer_addr })
                            .await
                        {
                            info!("ERROR in informing user about a new peer: {:?} - {}", e, e);
                        }

                        return Action::Continue(Some(bootstrap_group_ref));
                    } else if let Err(e) = self
                        .event_tx
                        .send(Event::ConnectedTo {
                            peer: Peer::Node(peer_addr),
                        })
                        .await
                    {
                        info!("ERROR in informing user about a new peer: {:?} - {}", e, e);
                    };
                }

                Action::Continue(None)
            } else {
                Action::HandleDuplicate(q_conn)
            }
        };

        match state {
            Action::HandleDuplicate(_q_conn) => {
                debug!("Not allowing duplicate connection from peer: {}", peer_addr);
            }
            Action::HandleAlreadyBootstrapped => self.handle_communication_err(
                peer_addr,
                &QuicP2pError::ConnectionCancelled,
                "Connection already bootstrapped",
                None,
            ),
            Action::Continue(bootstrap_group) => {
                if let Some(bootstrap_group_ref) = bootstrap_group {
                    bootstrap_group_ref.terminate_group(true, self.connections);
                }
                self.read_from_peer(peer_addr, uni_streams, bi_streams);
            }
        }
    }
}
