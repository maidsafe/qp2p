#![cfg(feature = "upnp")]

use qp2p::{Error, QuicP2p, Config};
use std::net::{IpAddr, Ipv4Addr};
use common::new_qp2p;

mod common;



#[tokio::test]
async fn echo_service() -> Result<(), Error> {
    let qp2p = QuicP2p::with_config(
        Some(Config {
            port: Some(0),
            ip: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
            ..Default::default()
        }),
        Default::default(),
        false,
    )?;
    let peer1 = qp2p.new_endpoint()?;
    let peer_addr = peer1.local_addr()?;

    let qp2p = QuicP2p::with_config(
        Some(Config {
            port: Some(0),
            ip: None,
            ..Default::default()
        }),
        vec![peer_addr].into(),
        false,
    )?;
    
    let mut peer2 = qp2p.new_endpoint()?;
    let addr = peer2.our_endpoint().await?;
    
    Ok(())
}