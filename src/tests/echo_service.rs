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

    // Create another endpoint and send an EchoServiceReq
    let mut peer2 = qp2p.new_endpoint()?;
    let socket_addr = peer2.socket_addr().await?;
    let connection = peer2.connect_to(&peer1_addr).await?;
    let (mut send_stream, mut recv_stream) = connection.open_bi_stream().await?;
    let echo_service_req = WireMsg::EndpointEchoReq;
    echo_service_req
        .write_to_stream(&mut send_stream.quinn_send_stream)
        .await?;

    // Listen for connections and incoming messages at peer 1
    let mut incoming = peer1.listen()?;
    let mut inbound_messages = incoming
        .next()
        .await
        .ok_or(Error::Unexpected("No incoming messages".to_string()))?;
    let message = inbound_messages.next().await;
    assert!(message.is_none()); // Qp2p  would handle this message internally

    let response = WireMsg::read_from_stream(&mut recv_stream.quinn_recv_stream).await?;
    match response {
        WireMsg::EndpointEchoResp(result) => {
            assert_eq!(socket_addr, result);
        }
        res => {
            return Err(Error::Unexpected(format!(
                "Unexpected response after echo service request: {}",
                res
            )))
        }
    }
    Ok(())
}
