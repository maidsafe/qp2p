use crate::context::{ctx_mut, FromPeer, ToPeer};
use crate::error::Error;
use crate::event::Event;
use crate::wire_msg::WireMsg;
use crate::{communicate, CrustInfo, R};
use std::collections::hash_map::Entry;
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::{fmt, mem};
use tokio::prelude::Future;
use tokio::runtime::current_thread;

/// Connect to the give peer
pub fn connect_to(peer_info: CrustInfo, send_after_connect: Option<WireMsg>) -> R<()> {
    let peer_cert = quinn::Certificate::from_der(&peer_info.peer_cert_der)?;
    let peer_addr = peer_info.peer_addr;

    let mut peer_cfg_builder = quinn::ClientConfigBuilder::new();
    unwrap!(peer_cfg_builder.add_certificate_authority(peer_cert));
    let peer_cfg = peer_cfg_builder.build();

    let r = ctx_mut(|c| {
        let conn = c
            .connections
            .entry(peer_addr)
            .or_insert_with(|| Default::default());
        if conn.to_peer.is_no_connection() {
            let mut pending_sends: VecDeque<_> = Default::default();
            if let Some(pending_send) = send_after_connect {
                pending_sends.push_back(pending_send);
            }
            conn.to_peer = ToPeer::Initiated {
                peer_cert_der: peer_info.peer_cert_der,
                pending_sends,
            };
            let peer_addr = peer_addr;
            c.quic_ep()
                .connect_with(&peer_cfg, &peer_addr, "MaidSAFE.net")
                .map_err(Error::from)
                .and_then(move |new_client_conn_fut| {
                    let leaf = new_client_conn_fut.then(move |new_client_conn_res| {
                        handle_new_connection_res(peer_addr, new_client_conn_res);
                        Ok(())
                    });

                    current_thread::spawn(leaf);
                    Ok(())
                })
        } else {
            Err(Error::DuplicateConnectionToPeer(peer_addr))
        }
    });

    if let Err(e) = r.as_ref() {
        // FIXME if you try to connect twice it will sever all existing connections too. This is
        // very aggressive - improve this
        handle_connect_err(peer_addr, e);
    }

    r
}

fn handle_new_connection_res(
    peer_addr: SocketAddr,
    new_client_conn_res: Result<quinn::NewClientConnection, quinn::ConnectionError>,
) {
    match new_client_conn_res {
        Ok(new_client_conn) => {
            println!("Successfully connected to peer: {}", peer_addr);
            ctx_mut(|c| {
                let conn = unwrap!(
                    c.connections.get_mut(&peer_addr),
                    "Logic Error in bookkeeping - Report a bug!"
                );

                let to_peer_prev = mem::replace(&mut conn.to_peer, Default::default());
                conn.to_peer = match to_peer_prev {
                    ToPeer::Initiated {
                        peer_cert_der,
                        pending_sends,
                    } => {
                        let q_conn = new_client_conn.connection;
                        match conn.from_peer {
                            FromPeer::NoConnection => communicate::write_to_peer_connection(
                                peer_addr,
                                &q_conn,
                                &WireMsg::CertificateDer(c.our_cert_der.clone()),
                            ),
                            FromPeer::Established {
                                ref mut pending_reads,
                                ..
                            } => {
                                for pending_read in pending_reads.drain(..) {
                                    communicate::dispatch_wire_msg(
                                        peer_addr,
                                        &q_conn,
                                        c.our_ext_addr_tx.take(),
                                        pending_read,
                                    );
                                }
                            }
                        }

                        for pending_send in &pending_sends {
                            communicate::write_to_peer_connection(peer_addr, &q_conn, pending_send);
                        }
                        ToPeer::Established {
                            peer_cert_der,
                            q_conn,
                        }
                    }
                    x => unreachable!(
                        "We can handle new connection only because it was previously \
                         initiated: {:?}",
                        x
                    ),
                };

                if let Err(e) = c.event_tx.send(Event::ConnectedTo { peer_addr }) {
                    println!("Could not fire event: {:?}", e);
                }
            })
        }
        Err(e) => handle_connect_err(peer_addr, e),
    }
}

fn handle_connect_err<E: fmt::Debug>(peer_addr: SocketAddr, err: E) {
    println!("Error connecting to peer {}: {:?}", peer_addr, err);
    ctx_mut(|c| {
        unwrap!(c.event_tx.send(Event::ConnectionFailure { peer_addr }));
        if let Entry::Occupied(oe) = c.connections.entry(peer_addr) {
            let conn = oe.remove();
            if !conn.from_peer.is_no_connection() {
                println!(
                    "Peer {} has a connection to us but we couldn't connect to it. \
                     All connections to this peer will now be severed.",
                    peer_addr
                );
            }
            // TODO: Consider closing the conn
        }
    })
}
