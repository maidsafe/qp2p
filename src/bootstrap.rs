// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::connect;
use crate::connection::BootstrapGroupMaker;
use crate::context::ctx;

pub fn start() {
    let (bootstrap_nodes, event_tx): (Vec<_>, _) = ctx(|c| {
        (
            c.bootstrap_cache
                .peers()
                .iter()
                .rev()
                .chain(c.bootstrap_cache.hard_coded_contacts().iter())
                .cloned()
                .collect(),
            c.event_tx.clone(),
        )
    });

    let maker = BootstrapGroupMaker::new(event_tx);
    for bootstrap_node in bootstrap_nodes {
        let _ = connect::connect_to(bootstrap_node, None, Some(&maker));
    }
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{new_random_qp2p, new_unbounded_channels, EventReceivers};
    use crate::{Builder, Config, Event, OurType, QuicP2p};
    use std::collections::{HashSet, VecDeque};
    use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
    use std::thread;
    use std::time::Duration;
    use unwrap::unwrap;

    // Try to bootstrap to 4 different peer sequentially. Use an artificial delay for 3 of the peers.
    // Make sure in the end we have only one bootstrapped connection (all other connections are
    // supposed to be dropped).
    #[test]
    fn bootstrap_to_only_one_node_with_delays() {
        test_bootstrap_to_multiple_peers(4, Some(10));
    }

    // Try to bootstrap to 4 different peer at once.
    // Make sure in the end we have only one bootstrapped connection (all other connections are
    // supposed to be dropped).
    #[test]
    fn bootstrap_to_only_one_node_without_delays() {
        test_bootstrap_to_multiple_peers(4, None);
    }

    fn test_bootstrap_to_multiple_peers(num_conns: u64, delay_ms: Option<u64>) {
        let mut bs_nodes = Vec::with_capacity(num_conns as usize);
        let mut bs_peer_addrs = HashSet::with_capacity(num_conns as usize);
        let mut hcc_contacts = HashSet::with_capacity(num_conns as usize);

        // Spin up 4 peers that we'll use for hardcoded contacts.
        for i in 0..num_conns {
            let (mut qp2p, rx) = new_random_qp2p(false, Default::default());
            if let Some(delay_ms) = delay_ms {
                qp2p.set_connect_delay(i * delay_ms);
            }

            let our_addr = unwrap!(qp2p.our_connection_info());

            assert!(bs_peer_addrs.insert(our_addr));
            assert!(hcc_contacts.insert(our_addr));
            bs_nodes.push((rx, qp2p));
        }

        // Create a client and bootstrap to 4 of the peers we created previously.
        let (mut qp2p0, rx0) = new_random_qp2p(false, hcc_contacts);
        qp2p0.bootstrap();

        match unwrap!(rx0.recv()) {
            Event::BootstrappedTo { node, .. } => {
                assert!(bs_peer_addrs.contains(&node));
            }
            ev => panic!("Unexpected event: {:?}", ev),
        }

        thread::sleep(Duration::from_millis(if let Some(delay_ms) = delay_ms {
            num_conns * delay_ms + delay_ms
        } else {
            100
        }));

        // We expect only 1 bootstrap connection to succeed.
        let conns_count = unwrap!(qp2p0.connections(|c| {
            c.iter()
                .filter(|(_pa, conn)| conn.to_peer.is_established())
                .count()
        }));
        assert_eq!(conns_count, 1);
    }

    // Try to bootstrap to hardcoded contacts and cached peers at once.
    // Bootstrap cache should be prioritised in this case.
    #[test]
    fn bootstrap_prioritise_cache() {
        // Total number of nodes (half for hardcoded contacts, half for the bootstrap cache).
        const NODES_COUNT: u64 = 8;

        // Initialise nodes for the bootstrap cache and for hardcoded contacts
        let mut hcc_nodes = Vec::with_capacity(NODES_COUNT as usize / 2);
        let mut bootstrap_cache_nodes = Vec::with_capacity(NODES_COUNT as usize / 2);

        for _i in 0..(NODES_COUNT / 2) {
            let (qp2p, rx) = test_node();
            bootstrap_cache_nodes.push((qp2p, rx));

            let (qp2p, rx) = test_node();
            hcc_nodes.push((qp2p, rx));
        }

        let bootstrap_cache: VecDeque<_> = bootstrap_cache_nodes
            .iter_mut()
            .map(|(node, _)| unwrap!(node.our_connection_info()))
            .collect();

        let hard_coded_contacts: HashSet<_> = hcc_nodes
            .iter_mut()
            .map(|(node, _)| unwrap!(node.our_connection_info()))
            .collect();

        // Waiting for all nodes to start
        let (ev_tx, ev_rx) = new_unbounded_channels();
        let mut bootstrapping_node = unwrap!(Builder::new(ev_tx)
            .with_config(Config {
                port: Some(0),
                ip: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
                hard_coded_contacts,
                our_type: OurType::Client,
                ..Default::default()
            })
            .with_bootstrap_nodes(bootstrap_cache.clone(), true,)
            .build());

        bootstrapping_node.bootstrap();

        for event in ev_rx.iter() {
            if let Event::BootstrappedTo { .. } = event {
                let attempted_conns = unwrap!(bootstrapping_node.attempted_connections());
                assert_eq!(
                    &attempted_conns[0..(NODES_COUNT as usize / 2)],
                    bootstrap_cache
                        .iter()
                        .rev()
                        .copied()
                        .collect::<Vec<_>>()
                        .as_slice()
                );
                break;
            }
        }
    }

    // Test that bootstrap fails after a handshake timeout if none of the peers
    // that we're bootstrapping to have responded.
    #[test]
    #[ignore] // FIXME: Modify test suite to delay events
    fn bootstrap_failure() {
        let (mut bootstrap_node, _rx) = test_node();
        bootstrap_node.set_connect_delay(100);

        let bootstrap_ci = unwrap!(bootstrap_node.our_connection_info());

        let mut hcc = HashSet::with_capacity(1);
        assert!(hcc.insert(bootstrap_ci));

        let (ev_tx, ev_rx) = new_unbounded_channels();
        let mut bootstrap_client = unwrap!(Builder::new(ev_tx)
            .with_config(Config {
                port: Some(0),
                ip: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
                hard_coded_contacts: hcc,
                our_type: OurType::Node,
                idle_timeout_msec: Some(30),
                ..Default::default()
            })
            .with_bootstrap_nodes(Default::default(), true)
            .build());

        bootstrap_client.bootstrap();

        match unwrap!(ev_rx.recv()) {
            Event::BootstrapFailure => (),
            ev => {
                panic!("Unexpected event: {:?}", ev);
            }
        }
    }

    #[test]
    fn node_will_attempt_hard_coded_contacts() {
        let (mut peer1, _) = test_node();
        let peer1_conn_info = unwrap!(peer1.our_connection_info());

        let (mut peer2, ev_rx) = {
            let mut hcc = HashSet::new();
            assert!(hcc.insert(peer1_conn_info.clone()));
            test_peer_with_hcc(hcc, OurType::Node)
        };

        peer2.bootstrap();
        let peer2_conn_info = unwrap!(peer2.our_connection_info());

        for event in ev_rx.iter() {
            if let Event::BootstrappedTo { node } = event {
                assert_eq!(node, peer1_conn_info);
                break;
            }
        }

        let is_peer1_state_valid = unwrap!(peer1.connections(move |c| {
            c[&peer2_conn_info].from_peer.is_established()
                && c[&peer2_conn_info].to_peer.is_established()
        }));
        assert!(is_peer1_state_valid);

        let is_peer2_state_valid = unwrap!(peer2.connections(move |c| {
            c[&peer1_conn_info].to_peer.is_established()
                && c[&peer1_conn_info].from_peer.is_established()
        }));
        assert!(is_peer2_state_valid);
    }

    #[test]
    fn client_will_attempt_hard_coded_contacts() {
        let (mut peer1, _) = test_node();
        let peer1_conn_info = unwrap!(peer1.our_connection_info());

        let (mut peer2, ev_rx) = {
            let mut hcc = HashSet::new();
            assert!(hcc.insert(peer1_conn_info.clone()));
            test_peer_with_hcc(hcc, OurType::Client)
        };

        peer2.bootstrap();
        let peer2_conn_info = unwrap!(peer2.our_connection_info());

        for event in ev_rx.iter() {
            if let Event::BootstrappedTo { node } = event {
                assert_eq!(node, peer1_conn_info);
                break;
            }
        }

        let is_peer1_state_valid = unwrap!(peer1.connections(move |c| {
            c[&peer2_conn_info].from_peer.is_established()
                && c[&peer2_conn_info].to_peer.is_not_needed()
        }));
        assert!(is_peer1_state_valid);

        let is_peer2_state_valid = unwrap!(peer2.connections(move |c| {
            c[&peer1_conn_info].to_peer.is_established()
                && c[&peer1_conn_info].from_peer.is_not_needed()
        }));
        assert!(is_peer2_state_valid);
    }

    #[test]
    fn node_will_attempt_cached_peers() {
        let (mut peer1, _) = test_node();
        let peer1_addr = unwrap!(peer1.our_connection_info());

        let (mut peer2, ev_rx) = test_peer_with_bootstrap_cache(vec![peer1_addr]);

        peer2.bootstrap();

        for event in ev_rx.iter() {
            if let Event::BootstrappedTo { node } = event {
                assert_eq!(node, peer1_addr);
                break;
            }
        }
    }

    #[test]
    fn node_will_report_failure_when_bootstrap_cache_and_hard_coded_contacts_are_empty() {
        let (mut peer, ev_rx) = test_node();

        peer.bootstrap();

        for event in ev_rx.iter() {
            if let Event::BootstrapFailure = event {
                break;
            }
        }
    }

    #[test]
    fn node_will_report_failure_when_bootstrap_cache_and_hard_coded_contacts_are_invalid() {
        let dummy_node_addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 37692));
        let (mut peer, ev_rx) = {
            let mut hcc = HashSet::new();
            assert!(hcc.insert(dummy_node_addr));
            test_peer_with_hcc(hcc, OurType::Node)
        };

        peer.bootstrap();

        for event in ev_rx.iter() {
            if let Event::BootstrapFailure = event {
                break;
            }
        }
    }

    fn test_peer_with_bootstrap_cache(
        mut cached_peers: Vec<SocketAddr>,
    ) -> (QuicP2p, EventReceivers) {
        let cached_peers: VecDeque<_> = cached_peers.drain(..).collect();
        let (ev_tx, ev_rx) = new_unbounded_channels();
        let builder = Builder::new(ev_tx)
            .with_config(Config {
                port: Some(0),
                ip: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
                ..Default::default()
            })
            .with_bootstrap_nodes(cached_peers, true);
        (unwrap!(builder.build()), ev_rx)
    }

    /// Constructs a `QuicP2p` node with some sane defaults for testing.
    fn test_node() -> (QuicP2p, EventReceivers) {
        test_peer_with_hcc(Default::default(), OurType::Node)
    }

    /// Construct a `QuicP2p` instance with optional set of hardcoded contacts.
    fn test_peer_with_hcc(
        hard_coded_contacts: HashSet<SocketAddr>,
        our_type: OurType,
    ) -> (QuicP2p, EventReceivers) {
        let (ev_tx, ev_rx) = new_unbounded_channels();
        let builder = Builder::new(ev_tx)
            .with_config(Config {
                port: Some(0),
                ip: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
                hard_coded_contacts,
                our_type,
                ..Default::default()
            })
            // Make sure we start with an empty cache. Otherwise, we might get into unexpected state.
            .with_bootstrap_nodes(Default::default(), true);
        (unwrap!(builder.build()), ev_rx)
    }
}
