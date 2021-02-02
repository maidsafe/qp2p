use crate::{api::Message, utils, Config, Endpoint, QuicP2p};
use anyhow::{anyhow, Result};
use assert_matches::assert_matches;
use bytes::Bytes;
use futures::future;
use quinn::EndpointError;
use std::{
    collections::HashSet,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    thread,
    time::Duration,
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

// Helper function that waits for an incoming connection.
// After 3 attempts, if no incoming connection is reported it returns None.
pub async fn get_incoming_connection(listening_peer: &mut Endpoint) -> Option<SocketAddr> {
    let mut attempts = 0;
    loop {
        if let Some(connecting_peer) = listening_peer.next_incoming_connection().await {
            return Some(connecting_peer);
        }
        thread::sleep(Duration::from_secs(2));
        attempts += 1;
        if attempts > 2 {
            return None;
        }
    }
}

pub async fn get_disconnection_event(listening_peer: &mut Endpoint) -> Option<SocketAddr> {
    let mut attempts = 0;
    loop {
        if let Some(connecting_peer) = listening_peer.next_disconnected_peer().await {
            return Some(connecting_peer);
        }
        thread::sleep(Duration::from_secs(2));
        attempts += 1;
        if attempts > 2 {
            return None;
        }
    }
}

// Helper function that listens for incoming messages
// After 3 attemps if no message has arrived it returns None.
pub async fn get_incoming_message(listening_peer: &mut Endpoint) -> Option<(SocketAddr, Bytes)> {
    let mut attempts = 0;
    loop {
        if let Some((source, message)) = listening_peer.next_incoming_message().await {
            return Some((source, message));
        }
        thread::sleep(Duration::from_secs(2));
        attempts += 1;
        if attempts > 2 {
            return None;
        }
    }
}