use crate::{Config, Error, QuicP2p};

#[tokio::test]
// #[ignore = "Will fail due to potential lack of hairpinning"]
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
    let peer_addr = peer1.socket_addr().await?;

    // Another EP builder with bootstrap node
    let qp2p = QuicP2p::with_config(
        Some(Config {
            port: Some(54321),
            ip: None,
            ..Default::default()
        }),
        &[peer_addr],
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
        let socket_addr = peer2.socket_addr().await?;
        Ok::<_, Error>(socket_addr)
    });
    handle1
        .await
        .map_err(|e| Error::Unexpected(format!("Error: {}", e)))??;
    let _echo_service_res = handle2
        .await
        .map_err(|e| Error::Unexpected(format!("Error: {}", e)))??;
    rt.shutdown_timeout(std::time::Duration::from_secs(10));
    Ok(())
}
