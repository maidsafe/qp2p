use crossbeam_channel as mpmc;
use quic_p2p::{Builder, Config, Event, EventSenders, OurType, Peer, QuicP2p};
use std::{
    collections::HashSet,
    net::{IpAddr, Ipv4Addr, SocketAddr},
};
use unwrap::unwrap;

/// Waits for `Event::ConnectedTo`.
fn wait_till_connected(ev_rx: EventReceivers) -> Peer {
    for event in ev_rx.iter() {
        if let Event::ConnectedTo { peer } = event {
            return peer;
        }
    }
    panic!("Didn't receive the expected ConnectedTo event");
}

struct EventReceivers {
    pub node_rx: mpmc::Receiver<Event>,
    pub client_rx: mpmc::Receiver<Event>,
}

impl EventReceivers {
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

fn new_unbounded_channels() -> (EventSenders, EventReceivers) {
    let (client_tx, client_rx) = mpmc::unbounded();
    let (node_tx, node_rx) = mpmc::unbounded();
    (
        EventSenders { node_tx, client_tx },
        EventReceivers { node_rx, client_rx },
    )
}

/// Constructs a `QuicP2p` node with some sane defaults for testing.
fn test_node() -> (QuicP2p, EventReceivers) {
    test_peer_with_hcc(Default::default(), OurType::Node)
}

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

#[test]
fn successfull_connection_stores_peer_in_bootstrap_cache() {
    let (mut peer1, _) = test_node();
    let peer1_addr = unwrap!(peer1.our_connection_info());

    let (mut peer2, ev_rx) = test_node();
    peer2.connect_to(peer1_addr);

    let connected_to = wait_till_connected(ev_rx);
    assert_eq!(connected_to, Peer::Node(peer1_addr));

    let cache = unwrap!(peer2.bootstrap_cache());
    assert_eq!(cache, vec![peer1_addr]);
}

#[test]
fn incoming_connections_yield_connected_to_event() {
    let (mut peer1, ev_rx) = test_node();
    let peer1_addr = unwrap!(peer1.our_connection_info());

    let (mut peer2, _) = test_node();
    peer2.connect_to(peer1_addr);
    let peer2_addr = unwrap!(peer2.our_connection_info());

    let peer = wait_till_connected(ev_rx);
    assert_eq!(peer.peer_addr(), peer2_addr);
}

#[test]
fn incoming_connections_are_not_put_into_bootstrap_cache_upon_connected_to_event() {
    let (mut peer1, ev_rx) = test_node();
    let peer1_conn_info = unwrap!(peer1.our_connection_info());

    let (mut peer2, _) = test_node();
    peer2.connect_to(peer1_conn_info);

    let _ = wait_till_connected(ev_rx);

    let cache = unwrap!(peer1.bootstrap_cache());
    assert!(cache.is_empty());
}
