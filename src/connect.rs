use crate::context::{ctx, ctx_mut, Connection, FromPeer, ToPeer};
use crate::error::Error;
use crate::event::Event;
use crate::wire_msg::WireMsg;
use crate::{communicate, CrustInfo, R};
use std::mem;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::prelude::Future;
use tokio::runtime::current_thread;

/// Connect to the give peer
pub fn connect_to(peer_info: CrustInfo, send_after_connect: Option<WireMsg>) -> R<()> {
    let peer_cert = quinn::Certificate::from_der(&peer_info.peer_cert_der)?;
    let peer_addr = peer_info.peer_addr;

    let peer_cfg = {
        let mut peer_cfg_builder = quinn::ClientConfigBuilder::new();
        if let Err(e) = peer_cfg_builder.add_certificate_authority(peer_cert) {
            let e = From::from(e);
            handle_connect_err(peer_addr, &e);
            return Err(e);
        }
        let mut peer_cfg = peer_cfg_builder.build();
        let transport_config = unwrap!(Arc::get_mut(&mut peer_cfg.transport));
        // TODO test that this is sent only over the uni-stream to the peer not on the uni
        // stream from the peer
        transport_config.idle_timeout = ctx(|c| c.idle_timeout);
        transport_config.keep_alive_interval = ctx(|c| c.keep_alive_interval);

        peer_cfg
    };

    let r = ctx_mut(|c| {
        let event_tx = c.event_tx.clone();
        let conn = c
            .connections
            .entry(peer_addr)
            .or_insert_with(|| Connection::new(peer_addr, event_tx));
        if conn.to_peer.is_no_connection() {
            let mut pending_sends: Vec<_> = Default::default();
            if let Some(pending_send) = send_after_connect {
                pending_sends.push(pending_send);
            }
            conn.to_peer = ToPeer::Initiated {
                peer_cert_der: peer_info.peer_cert_der,
                pending_sends,
            };
            let peer_addr = peer_addr;
            c.quic_ep()
                .connect_with(&peer_cfg, &peer_addr, "MaidSAFE.net")
                .map_err(Error::from)
                .map(move |new_client_conn_fut| {
                    let leaf = new_client_conn_fut.then(move |new_peer_conn_res| {
                        handle_new_connection_res(peer_addr, new_peer_conn_res);
                        Ok(())
                    });

                    current_thread::spawn(leaf);
                })
        } else {
            Err(Error::DuplicateConnectionToPeer(peer_addr))
        }
    });

    if let Err(e) = r.as_ref() {
        handle_connect_err(peer_addr, e);
    }

    r
}

fn handle_new_connection_res(
    peer_addr: SocketAddr,
    new_peer_conn_res: Result<quinn::NewClientConnection, quinn::ConnectionError>,
) {
    let new_peer_conn = match new_peer_conn_res {
        Ok(new_peer_conn) => new_peer_conn,
        Err(e) => return handle_connect_err(peer_addr, &From::from(e)),
    };

    println!("Successfully connected to peer: {}", peer_addr);

    ctx_mut(|c| {
        let conn = unwrap!(
            c.connections.get_mut(&peer_addr),
            "Logic Error in bookkeeping - Report a bug!"
        );

        let mut to_peer_prev = mem::replace(&mut conn.to_peer, Default::default());
        let (peer_cert_der, pending_sends) = match to_peer_prev {
            ToPeer::Initiated {
                ref mut peer_cert_der,
                ref mut pending_sends,
            } => (
                mem::replace(peer_cert_der, Default::default()),
                mem::replace(pending_sends, Default::default()),
            ),
            x => unreachable!(
                "We can handle new connection only because it was previously \
                 initiated: {:?}",
                x
            ),
        };

        let q_conn = new_peer_conn.connection;
        match conn.from_peer {
            FromPeer::NoConnection => {
                communicate::write_to_peer_connection(
                    peer_addr,
                    &q_conn,
                    &WireMsg::CertificateDer(c.our_complete_cert.cert_der.clone()),
                );
            }
            FromPeer::Established {
                ref mut pending_reads,
                ..
            } => {
                let crust_info = CrustInfo {
                    peer_addr,
                    peer_cert_der: peer_cert_der.clone(),
                };

                if let Err(e) = c.event_tx.send(Event::ConnectedTo { crust_info }) {
                    println!("Could not fire event: {:?}", e);
                }

                for pending_read in pending_reads.drain(..) {
                    communicate::dispatch_wire_msg(
                        peer_addr,
                        &q_conn,
                        c.our_ext_addr_tx.take(),
                        &c.event_tx,
                        pending_read,
                    );
                }
            }
        }

        for pending_send in &pending_sends {
            communicate::write_to_peer_connection(peer_addr, &q_conn, pending_send);
        }

        conn.to_peer = ToPeer::Established {
            peer_cert_der,
            q_conn,
        };
    })
}

fn handle_connect_err(peer_addr: SocketAddr, e: &Error) {
    println!(
        "Error connecting to peer {}: {:?} - Details: {}",
        peer_addr, e, e
    );

    if let Error::DuplicateConnectionToPeer(_) = e {
        return;
    }

    ctx_mut(|c| {
        if let Some(conn) = c.connections.remove(&peer_addr) {
            if !conn.from_peer.is_no_connection() {
                println!(
                    "Peer {} has a connection to us but we couldn't connect to it. \
                     All connections to this peer will now be severed.",
                    peer_addr
                );
            }
        }
    })
}
