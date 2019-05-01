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
    let (proxies, event_tx): (Vec<_>, _) = ctx(|c| {
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
    for proxy in proxies {
        let _ = connect::connect_to(proxy, None, Some(&maker));
    }
}

#[cfg(test)]
mod tests {
    use crate::test_utils::new_random_qp2p_for_unit_test;
    use crate::{Builder, Config, Event};
    use std::collections::HashSet;
    use std::net::{IpAddr, Ipv4Addr};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

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
            let (tx, rx) = mpsc::channel();
            let mut qp2p = unwrap!(Builder::new(tx)
                .with_config(Config {
                    ip: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
                    port: Some(0),
                    ..Default::default()
                })
                .build());

            if let Some(delay_ms) = delay_ms {
                qp2p.set_connect_delay(i * delay_ms);
            }

            let ci_info = unwrap!(qp2p.our_connection_info());

            bs_peer_addrs.insert(ci_info.peer_addr);
            hcc_contacts.insert(ci_info);
            bs_nodes.push((rx, qp2p));
        }

        // Create a client and bootstrap to 4 of the peers we created previously.
        let (mut qp2p0, rx0) = new_random_qp2p_for_unit_test(false, hcc_contacts);
        qp2p0.bootstrap();

        match unwrap!(rx0.recv()) {
            Event::BootstrappedTo { node, .. } => {
                assert!(bs_peer_addrs.contains(&node.peer_addr));
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
}
