#![cfg(feature = "upnp")]

use common::new_qp2p;
use qp2p::{Config, Error, QuicP2p};
use std::net::{IpAddr, Ipv4Addr};

mod common;

#[tokio::test]
async fn echo_service() -> Result<(), Error> {
    let qp2p = QuicP2p::with_config(
        Some(Config {
            port: Some(12345),
            ip: None,
            ..Default::default()
        }),
        Default::default(),
        false,
    )?;
    let mut peer1 = qp2p.new_endpoint()?;
    let peer_addr = peer1.our_endpoint().await?;
    dbg!(&peer_addr);

    let qp2p = QuicP2p::with_config(
        Some(Config {
            port: Some(54321),
            ip: None,
            ..Default::default()
        }),
        vec![peer_addr].into(),
        false,
    )?;

    let rt = tokio::runtime::Runtime::new()?;
    let handle1 = rt.spawn(async move {
        let mut incoming = peer1.listen()?;
        println!("Listening...");
        let mut inbound_messages = incoming
            .next()
            .await
            .ok_or(Error::Unexpected("No incoming messages".to_string()))?;
        println!("Got inbound messges");
        let message = inbound_messages.next().await;
        assert!(message.is_none()); // Qp2p would handle the message interally
        Ok::<(), Error>(())
    });

    let handle2 = rt.spawn(async move {
        let mut peer2 = qp2p.new_endpoint()?;
        let addr_future = peer2.our_endpoint();
        let socket_addr = addr_future.await?;
        dbg!(&socket_addr);
        Ok::<_, Error>(socket_addr)
    });
    let res2 = handle2
        .await
        .map_err(|e| Error::Unexpected(format!("Error: {}", e)))?;
    let res = handle1
        .await
        .map_err(|e| Error::Unexpected(format!("Error: {}", e)))??;
    dbg!(res2?);
    let x = rt.shutdown_background();
    Ok(())
}
