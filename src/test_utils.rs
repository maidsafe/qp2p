// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::config::Config;
use crate::connection::{Connection, FromPeer, QConn, ToPeer};
use crate::context::ctx;
use crate::context::Context;
use crate::dirs::{Dirs, OverRide};
use crate::event::Event;
use crate::utils::{Token, R};
use crate::wire_msg::WireMsg;
use crate::{communicate, Builder, EventSenders, Peer, QuicP2p};
use crossbeam_channel as mpmc;
use futures::future::Future;
use rand::Rng;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::PathBuf;
use std::sync::mpsc;
use std::{env, ops::Deref, time::Duration};
use unwrap::unwrap;

thread_local! {
    static TEST_CTX: RefCell<Option<TestContext>> = RefCell::new(None);
}

/// Obtain a mutable referece to the `TestContext`. This initialise `TestContext` with a default value
/// if it has not been set in the current thread context.
fn test_ctx_mut<F, R>(f: F) -> R
where
    F: FnOnce(&mut TestContext) -> R,
{
    TEST_CTX.with(|ctx_refcell| {
        let mut ctx = ctx_refcell.borrow_mut();
        if let Some(ctx) = ctx.as_mut() {
            f(ctx)
        } else {
            let mut new_ctx = Default::default();
            let res = f(&mut new_ctx);
            *ctx = Some(new_ctx);
            res
        }
    })
}

/// Obtain a reference to the `TestContext`. This will use the default `TestContext` if it has not been set
/// in the current thread context.
fn test_ctx<F, R>(f: F) -> R
where
    F: FnOnce(&TestContext) -> R,
{
    TEST_CTX.with(|ctx_refcell| {
        let ctx = ctx_refcell.borrow();
        if let Some(ctx) = ctx.as_ref() {
            f(ctx)
        } else {
            f(&Default::default())
        }
    })
}

/// Newtype wrapper around `quinn::Endpoint` to allow creating artificial delays for outgoing connections.
pub(crate) struct EndpointWrap<'a>(&'a quinn::Endpoint);

impl<'a> EndpointWrap<'a> {
    /// Wraps around `quinn::Endpoint::connect_with` and optionally creates an artificial delay.
    /// Use `test_ctx` to set the delay.
    pub fn connect_with(
        &self,
        config: quinn::ClientConfig,
        addr: &SocketAddr,
        server_name: &str,
    ) -> Result<
        impl Future<Output = Result<quinn::NewConnection, quinn::ConnectionError>>,
        quinn::ConnectError,
    > {
        test_ctx_mut(|ctx| ctx.attempted_connections.push(addr.clone()));

        let connecting_res = self.0.connect_with(config, addr, server_name)?;
        let delay_ms = test_ctx(|ctx| ctx.connect_delay);
        Ok(async move {
            if delay_ms > 0 {
                tokio::time::delay_for(Duration::from_millis(delay_ms)).await;
            }
            connecting_res.await
        })
    }
}

impl<'a> Deref for EndpointWrap<'a> {
    type Target = quinn::Endpoint;

    fn deref(&self) -> &quinn::Endpoint {
        self.0
    }
}

impl Context {
    pub(crate) fn quic_ep(&self) -> EndpointWrap {
        EndpointWrap(&self.quic_ep)
    }
}

#[derive(Default)]
pub(crate) struct TestContext {
    connect_delay: u64,
    attempted_connections: Vec<SocketAddr>,
}

/// Extend `QuicP2p` with test functions.
impl QuicP2p {
    pub(crate) fn set_connect_delay(&mut self, delay_ms: u64) {
        self.el
            .post(move || test_ctx_mut(|ctx| ctx.connect_delay = delay_ms));
    }

    /// Get a list of attempted connections.
    pub(crate) fn attempted_connections(&mut self) -> R<Vec<SocketAddr>> {
        let (tx, rx) = mpsc::channel();
        self.el.post(move || {
            let res = test_ctx(|ctx| ctx.attempted_connections.clone());
            let _ = tx.send(res);
        });
        Ok(rx.recv()?)
    }

    /// Send an arbitrary message. Used for testing malicious nodes and clients.
    pub(crate) fn send_wire_msg(&mut self, peer: Peer, msg: WireMsg, token: Token) {
        self.el.post(move || {
            unwrap!(communicate::try_write_to_peer(peer, msg, token));
        });
    }

    /// Returns a list of connections of this peer.
    pub(crate) fn connections<F: Send + 'static, Res: Send + 'static>(&mut self, f: F) -> R<Res>
    where
        F: FnOnce(&HashMap<SocketAddr, Connection>) -> Res,
    {
        let (tx, rx) = mpsc::channel();
        self.el.post(move || {
            let res = ctx(|c| f(&c.connections));
            let _ = tx.send(res);
        });
        Ok(rx.recv()?)
    }
}

pub(crate) fn test_dirs() -> Dirs {
    Dirs::Overide(OverRide::new(&unwrap!(tmp_rand_dir().to_str())))
}

pub(crate) fn rand_node_addr() -> SocketAddr {
    let mut rng = rand::thread_rng();
    make_node_addr(rng.gen())
}

pub(crate) fn make_node_addr(port: u16) -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
}

fn tmp_rand_dir() -> PathBuf {
    let fname = format!("quic_p2p_tests_{:016x}", rand::random::<u64>());
    let mut path = env::temp_dir();
    path.push(fname);
    path
}

pub(crate) struct EventReceivers {
    pub node_rx: mpmc::Receiver<Event>,
    pub client_rx: mpmc::Receiver<Event>,
}

impl EventReceivers {
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

    pub fn try_recv(&self) -> Result<Event, mpmc::TryRecvError> {
        self.node_rx
            .try_recv()
            .or_else(|_| self.client_rx.try_recv())
    }

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

pub(crate) fn new_unbounded_channels() -> (EventSenders, EventReceivers) {
    let (client_tx, client_rx) = mpmc::unbounded();
    let (node_tx, node_rx) = mpmc::unbounded();
    (
        EventSenders { node_tx, client_tx },
        EventReceivers { node_rx, client_rx },
    )
}

/// Creates a new `QuicP2p` instance for testing.
pub(crate) fn new_random_qp2p(
    is_addr_unspecified: bool,
    contacts: HashSet<SocketAddr>,
) -> (QuicP2p, EventReceivers) {
    let (tx, rx) = new_unbounded_channels();
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
        let bi_stream = conn.open_bi();

        let leaf = async move {
            let mut o_stream = match bi_stream.await {
                Ok((o_stream, _i_stream)) => o_stream,
                Err(e) => panic!("Open-Bidirectional: {:?} {}", e, e),
            };
            let (message, msg_flag) = wire_msg.into();
            if let Err(e) = o_stream.write_all(&message[..]).await {
                panic!("Write-All: {:?} {}", e, e)
            };
            if let Err(e) = o_stream.write_all(&[msg_flag]).await {
                panic!("Write-All: {:?} {}", e, e)
            };
            if let Err(e) = o_stream.finish().await {
                panic!("Shutdown-after-write: {:?} {}", e, e);
            }
        };
        let _ = tokio::spawn(leaf);
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
