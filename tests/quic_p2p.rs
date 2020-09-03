use quic_p2p::{QuicP2p, Config};
use std::{
    collections::HashSet,
    net::{IpAddr, Ipv4Addr, SocketAddr},
};

/// Constructs a `QuicP2p` node with some sane defaults for testing.
fn new_quic_p2p() -> QuicP2p {
    new_quic_p2p_with_hcc(Default::default())
}

fn new_quic_p2p_with_hcc(
    hard_coded_contacts: HashSet<SocketAddr>,
) -> QuicP2p {
    QuicP2p::with_config(
        Some(Config {
            port: Some(0),
            ip: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
            hard_coded_contacts,
            ..Default::default()
        }),
        // Make sure we start with an empty cache. Otherwise, we might get into unexpected state.
        Default::default(),
        true,
    ).expect("Error creating QuicP2p object")
}

#[tokio::test]
async fn successful_connection() {
    let quic_p2p = new_quic_p2p();
    let peer1 = quic_p2p.new_endpoint().expect("Error creating endpoint 1");
    let peer1_addr = peer1.local_address();

    let peer2 = quic_p2p.new_endpoint().expect("Error creating endpoint 2");
    let _connection = peer2.connect_to(&peer1_addr).await.expect("Error creating connection between peers");

    let mut incoming_conn = peer1.listen().unwrap();
    let incoming_messages = incoming_conn.next().await.expect("Expected incoming connection");
    assert_eq!(incoming_messages.remote_addr(), peer2.local_address());
}
