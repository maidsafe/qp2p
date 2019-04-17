// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::bootstrap_cache::BootstrapCache;
use crate::config::{Config, OurType, SerialisableCertificate};
use crate::connection::{FromPeer, QConn, ToPeer};
use crate::context::{ctx, initialise_ctx, Context};
use crate::event::Event;
use crate::event_loop::EventLoop;
use crate::wire_msg::WireMsg;
use crate::{communicate, listener, peer_config, Builder, NodeInfo, Peer, QuicP2p, R};
use std::collections::HashSet;
use std::mem;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::mpsc::{self, Receiver, Sender};
use tokio::prelude::Future;
use tokio::runtime::current_thread;

/// Creates a new `QuicP2p` instance for testing.
pub(crate) fn new_random_qp2p_for_unit_test(
    is_addr_unspecified: bool,
    contacts: HashSet<NodeInfo>,
) -> (QuicP2p, Receiver<Event>) {
    let (tx, rx) = mpsc::channel();
    let qp2p = {
        let mut cfg = Config::with_default_cert();
        cfg.hard_coded_contacts = contacts;
        cfg.port = Some(0);
        if !is_addr_unspecified {
            cfg.ip = Some(IpAddr::V4(Ipv4Addr::LOCALHOST));
        }
        unwrap!(Builder::new(tx).with_config(cfg).build())
    };

    (qp2p, rx)
}

/// Connect and open a bi-directional stream.
/// This will fail if we don't have a connection to the peer or if the peer is in an invalid state
/// to be sent a message to.
pub(crate) fn write_to_bi_stream(peer_addr: SocketAddr, wire_msg: WireMsg) {
    fn write_to_bi(conn: &QConn, wire_msg: WireMsg) {
        let leaf = conn
            .open_bi()
            .map_err(move |e| panic!("Open-Bidirectional: {:?} {}", e, e))
            .and_then(move |(o_stream, _i_stream)| {
                tokio::io::write_all(o_stream, wire_msg.into())
                    .map_err(move |e| panic!("Write-All: {:?} {}", e, e))
            })
            .and_then(move |(o_stream, _): (_, bytes::Bytes)| {
                tokio::io::shutdown(o_stream)
                    .map_err(move |e| panic!("Shutdown-after-write: {:?} {}", e, e))
            })
            .map(|_| ());
        current_thread::spawn(leaf);
    }

    ctx(|c| {
        let conn = match c.connections.get(&peer_addr) {
            Some(conn) => conn,
            None => panic!("Asked to communicate with an unknown peer: {}", peer_addr),
        };

        match &conn.to_peer {
            ToPeer::NotNeeded => {
                if let FromPeer::Established { ref q_conn, .. } = conn.from_peer {
                    write_to_bi(q_conn, wire_msg);
                } else {
                    panic!(
                        "TODO We cannot communicate with someone we are not needing to connect to \
                         and they are not connected to us just now. Peer: {}",
                        peer_addr
                    );
                }
            }
            ToPeer::Established { ref q_conn, .. } => write_to_bi(q_conn, wire_msg),
            ToPeer::NoConnection | ToPeer::Initiated { .. } => {
                panic!(
                    "Peer {} is in invalid state {:?} to be communicated to",
                    peer_addr, conn.to_peer
                );
            }
        }
    })
}

/// Mock `QuicP2p`, used for testing malicious client scenarios.
pub(crate) struct TestClient {
    event_tx: Sender<Event>,
    el: EventLoop,
}

impl TestClient {
    /// Create a test client
    pub fn new(event_tx: Sender<Event>) -> Self {
        let el = EventLoop::spawn();
        Self { event_tx, el }
    }

    /// Must be called only once. There can only be one context per `TestClient` instance.
    pub fn activate(&mut self, our_type: OurType) -> R<()> {
        let tx = self.event_tx.clone();

        let ((key, cert), our_complete_cert) = {
            let our_complete_cert: SerialisableCertificate = Default::default();
            (
                our_complete_cert.obtain_priv_key_and_cert(),
                our_complete_cert,
            )
        };

        let (err_tx, err_rx) = mpsc::channel::<R<()>>();
        self.el.post(move || {
            let our_cfg = unwrap!(peer_config::new_our_cfg(
                peer_config::DEFAULT_IDLE_TIMEOUT_MSEC,
                peer_config::DEFAULT_KEEP_ALIVE_INTERVAL_MSEC,
                cert,
                key
            ));

            let mut ep_builder = quinn::Endpoint::builder();
            ep_builder.listen(our_cfg);
            let (dr, ep, incoming_connections) = {
                match UdpSocket::bind(&(Ipv4Addr::LOCALHOST, 0)) {
                    Ok(udp) => unwrap!(ep_builder.with_socket(udp)),
                    Err(e) => {
                        panic!("Could not bind a random port! Error: {:?}- {}", e, e);
                    }
                }
            };

            let mut bootstrap_cache =
                unwrap!(BootstrapCache::new(Default::default(), Default::default()));

            // Do not use the bootstrap cache.
            mem::replace(bootstrap_cache.peers_mut(), Default::default());

            let ctx = Context::new(
                tx,
                our_complete_cert,
                crate::DEFAULT_MAX_ALLOWED_MSG_SIZE,
                peer_config::DEFAULT_IDLE_TIMEOUT_MSEC,
                peer_config::DEFAULT_KEEP_ALIVE_INTERVAL_MSEC,
                our_type,
                bootstrap_cache,
                ep,
            );

            initialise_ctx(ctx);

            current_thread::spawn(dr.map_err(|e| warn!("Error in quinn Driver: {:?}", e)));

            if our_type != OurType::Client {
                listener::listen(incoming_connections);
            }

            let _ = err_tx.send(Ok(()));
        });

        (err_rx.recv()?)?;

        Ok(())
    }

    /// Send an arbitrary message.
    pub fn send(&mut self, peer: Peer, msg: WireMsg) {
        self.el.post(move || {
            communicate::try_write_to_peer(peer, msg);
        });
    }
}
