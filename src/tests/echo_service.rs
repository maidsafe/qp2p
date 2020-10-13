use crate::{wire_msg::WireMsg, Config, Error, QuicP2p, Result};

#[tokio::test]
async fn echo_service() -> Result<()> {
    // Endpoint builder
    let qp2p = QuicP2p::with_config(
        Some(Config {
            port: None,
            ip: None,
            ..Default::default()
        }),
        Default::default(),
        false,
    )?;
    // Create Endpoint
    let mut peer1 = qp2p.new_endpoint()?;
    let peer1_addr = peer1.socket_addr().await?;

    // Listen for messages / connections at peer 1
    let handle1 = tokio::spawn(async move {
        let mut incoming = peer1.listen()?;
        let mut inbound_messages = incoming
            .next()
            .await
            .ok_or_else(|| Error::Unexpected("No incoming messages".to_string()))?;
        let _message = inbound_messages.next().await;
        Ok::<_, Error>(inbound_messages) // Return this object to prevent the connection from being dropped
    });

    // In parallel create another endpoint and send an EchoServiceReq
    let handle2 = tokio::spawn(async move {
        let mut peer2 = qp2p.new_endpoint()?;
        let socket_addr = peer2.socket_addr().await?;
        let connection = peer2.connect_to(&peer1_addr).await?;
        let (mut send_stream, mut recv_stream) = connection.open_bi_stream().await?;
        let echo_service_req = WireMsg::EndpointEchoReq;
        echo_service_req
            .write_to_stream(&mut send_stream.quinn_send_stream)
            .await?;
        let response = WireMsg::read_from_stream(&mut recv_stream.quinn_recv_stream).await?;
        match response {
            WireMsg::EndpointEchoResp(result) => {
                assert_eq!(socket_addr, result);
                Ok::<(), Error>(())
            }
            res => Err(Error::Unexpected(format!(
                "Unexpected response after echo service request: {}",
                res
            ))),
        }
    });
    let (res1, res2) = futures::join!(handle1, handle2);
    let _ = res1??;
    res2??;
    Ok(())
}
