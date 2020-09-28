#![cfg(feature = "upnp")]

use common::new_qp2p;
use qp2p::{Config, Error, QuicP2p};
use std::net::{IpAddr, Ipv4Addr};

mod common;

#[tokio::test]
async fn echo_service() -> Result<(), Error> {
    // Endpoint builder
    let qp2p = QuicP2p::with_config(
        Some(Config {
            port: Some(12345),
            ip: None,
            ..Default::default()
        }),
        Default::default(),
        false,
    )?;
    // Create Endpoint
    let mut peer1 = qp2p.new_endpoint()?;
    // Get our address (using IGD only)
    let peer_addr = peer1.our_endpoint().await?;

    // Another EP builder with bootstrap node
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
    // Create thread where the for a peer to listen and respond to a 
    // EchoServiceRequest
    let handle1 = rt.spawn(async move {
        let mut incoming = peer1.listen()?;
        let mut inbound_messages = incoming
            .next()
            .await
            .ok_or(Error::Unexpected("No incoming messages".to_string()))?;
        let message = inbound_messages.next().await;
        assert!(message.is_none()); // Qp2p  would handle this message internally
        Ok::<(), Error>(())
    });

    // In parallel create another endpoint and 
    // get our address using echo service
    let handle2 = rt.spawn(async move {
        let mut peer2 = qp2p.new_endpoint()?;
        let addr_future = peer2.our_endpoint();
        let socket_addr = addr_future.await?;
        Ok::<_, Error>(socket_addr)
    });
    handle1
        .await
        .map_err(|e| Error::Unexpected(format!("Error: {}", e)))??;
    let echo_service_res = handle2
        .await
        .map_err(|e| Error::Unexpected(format!("Error: {}", e)))??;
    rt.shutdown_timeout(std::time::Duration::from_secs(10));
    assert_eq!(echo_service_res.port(), 54321);
    Ok(())
}
