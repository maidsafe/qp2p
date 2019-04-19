use quic_p2p::{Builder, Config, Event, NodeInfo, Peer, QuicP2p};
use std::collections::{HashSet, VecDeque};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::mpsc;
use unwrap::unwrap;

/// Waits for `Event::ConnectedTo`.
fn wait_till_connected(ev_rx: mpsc::Receiver<Event>) -> Peer {
    for event in ev_rx.iter() {
        if let Event::ConnectedTo { peer } = event {
            return peer;
        }
    }
    panic!("Didn't receive the expected ConnectodTo event");
}

/// Constructs `QuicP2p` instace with some sane defaults for testing.
fn test_peer() -> (QuicP2p, mpsc::Receiver<Event>) {
    test_peer_with_hcc(Default::default())
}

fn test_peer_with_hcc(hard_coded_contacts: HashSet<NodeInfo>) -> (QuicP2p, mpsc::Receiver<Event>) {
    let (ev_tx, ev_rx) = mpsc::channel();
    let builder = Builder::new(ev_tx)
        .with_config(Config {
            port: Some(0),
            ip: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
            hard_coded_contacts,
            ..Default::default()
        })
        // Make sure we start with an empty cache. Otherwise, we might get into unexpected state.
        .with_proxies(Default::default(), true);
    (unwrap!(builder.build()), ev_rx)
}

fn test_peer_with_bootstrap_cache(
    mut cached_peers: Vec<NodeInfo>,
) -> (QuicP2p, mpsc::Receiver<Event>) {
    let cached_peers: VecDeque<_> = cached_peers.drain(..).collect();
    let (ev_tx, ev_rx) = mpsc::channel();
    let builder = Builder::new(ev_tx)
        .with_config(Config {
            port: Some(0),
            ip: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
            ..Default::default()
        })
        .with_proxies(cached_peers, true);
    (unwrap!(builder.build()), ev_rx)
}

#[test]
fn successfull_connection_stores_peer_in_bootstrap_cache() {
    let (mut peer1, _) = test_peer();
    let peer1_conn_info = unwrap!(peer1.our_connection_info());

    let (mut peer2, ev_rx) = test_peer();
    peer2.connect_to(peer1_conn_info.clone());

    let connected_to = wait_till_connected(ev_rx);
    assert_eq!(connected_to, peer1_conn_info.clone().into());

    let cache = unwrap!(peer2.bootstrap_cache());
    assert_eq!(cache, vec![peer1_conn_info]);
}

#[test]
fn incoming_connections_yield_connected_to_event() {
    let (mut peer1, ev_rx) = test_peer();
    let peer1_conn_info = unwrap!(peer1.our_connection_info());

    let (mut peer2, _) = test_peer();
    peer2.connect_to(peer1_conn_info.clone());
    let peer2_conn_info = unwrap!(peer2.our_connection_info());

    let peer = wait_till_connected(ev_rx);
    assert_eq!(
        unwrap!(peer.peer_cert_der()),
        &peer2_conn_info.peer_cert_der[..]
    );
}

#[test]
fn incoming_connections_are_not_put_into_bootstrap_cache_upon_connected_to_event() {
    let (mut peer1, ev_rx) = test_peer();
    let peer1_conn_info = unwrap!(peer1.our_connection_info());

    let (mut peer2, _) = test_peer();
    peer2.connect_to(peer1_conn_info.clone());

    let _ = wait_till_connected(ev_rx);

    let cache = unwrap!(peer1.bootstrap_cache());
    assert!(cache.is_empty());
}

mod bootstrap {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn it_will_attempt_hard_coded_contacts() {
        let (mut peer1, _) = test_peer();
        let peer1_conn_info = unwrap!(peer1.our_connection_info());

        let (mut peer2, ev_rx) = {
            let mut hcc = HashSet::new();
            hcc.insert(peer1_conn_info.clone());
            test_peer_with_hcc(hcc)
        };

        peer2.bootstrap();

        for event in ev_rx.iter() {
            if let Event::BootstrappedTo { node } = event {
                assert_eq!(node, peer1_conn_info);
                break;
            }
        }
    }

    #[test]
    fn it_will_attempt_cached_peers() {
        let (mut peer1, _) = test_peer();
        let peer1_conn_info = unwrap!(peer1.our_connection_info());

        let (mut peer2, ev_rx) = test_peer_with_bootstrap_cache(vec![peer1_conn_info.clone()]);

        peer2.bootstrap();

        for event in ev_rx.iter() {
            if let Event::BootstrappedTo { node } = event {
                assert_eq!(node, peer1_conn_info);
                break;
            }
        }
    }

    #[test]
    fn it_will_report_failure_when_bootstrap_cache_and_hard_coded_contacts_are_empty() {
        let (mut peer, ev_rx) = test_peer();

        peer.bootstrap();

        for event in ev_rx.iter() {
            if let Event::BootstrapFailure = event {
                break;
            }
        }
    }

    #[test]
    fn it_will_report_failure_when_bootstrap_cache_and_hard_coded_contacts_are_invalid() {
        let dummy_peer_info = NodeInfo {
            peer_addr: SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 37692)),
            peer_cert_der: vec![1, 2, 3],
        };
        let (mut peer, ev_rx) = {
            let mut hcc = HashSet::new();
            hcc.insert(dummy_peer_info.clone());
            test_peer_with_hcc(hcc)
        };

        peer.bootstrap();

        for event in ev_rx.iter() {
            if let Event::BootstrapFailure = event {
                break;
            }
        }
    }
}
