use crate::{Config, QuicP2p};
use anyhow::Result;
use bytes::Bytes;
use std::{
    collections::HashSet,
    net::{IpAddr, Ipv4Addr, SocketAddr},
};

mod common;

/// Constructs a `QuicP2p` node with some sane defaults for testing.
pub fn new_qp2p() -> Result<QuicP2p> {
    new_qp2p_with_hcc(HashSet::default())
}

pub fn new_qp2p_with_hcc(hard_coded_contacts: HashSet<SocketAddr>) -> Result<QuicP2p> {
    let qp2p = QuicP2p::with_config(
        Some(Config {
            local_port: Some(0),
            local_ip: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
            hard_coded_contacts,
            ..Config::default()
        }),
        // Make sure we start with an empty cache. Otherwise, we might get into unexpected state.
        Default::default(),
        true,
    )?;

    Ok(qp2p)
}

pub fn random_msg() -> Bytes {
    let random_bytes: Vec<u8> = (0..1024).map(|_| rand::random::<u8>()).collect();
    Bytes::from(random_bytes)
}
