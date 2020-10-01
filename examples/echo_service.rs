use async_std::io;
use bytes::Bytes;
use qp2p::{Config, Error, Message, QuicP2p};
use std::env;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let args: Vec<String> = env::args().collect();
    let (bootstrap_nodes, genesis) = match &args[1][..] {
        "create" => (vec![], true),
        "connect" => {
            let bootstrap_node = args[2].parse().expect("SocketAddr format not recognized");
            (vec![bootstrap_node], false)
        }
        _ => panic!("Unexpected argument"),
    };
    let qp2p = QuicP2p::with_config(
        Some(Config {
            ip: None,
            port: Some(0),
            ..Default::default()
        }),
        &bootstrap_nodes,
        false,
    )?;
    let mut endpoint = qp2p.new_endpoint()?;
    let socket_addr = endpoint.our_endpoint().await?;
    println!("Process running at: {}", &socket_addr);
    if genesis {
        println!("Waiting for connections");
        let mut incoming = endpoint.listen()?;
        let mut messages = incoming
            .next()
            .await
            .ok_or_else(|| Error::Unexpected("Error during incoming connection".to_string()))?;
        let connecting_peer = messages.remote_addr();
        println!("Incoming connection from: {}", &connecting_peer);
        let message = messages.next().await;
        assert!(message.is_none());
        println!("Responded to peer with EchoService response");
        println!("Waiting for messages...");
        let mut messages = incoming
            .next()
            .await
            .ok_or_else(|| Error::Unexpected("Error during incoming connection".to_string()))?;
        let message = messages.next().await.unwrap();
        let (mut bytes, mut send, mut recv) = if let Message::BiStream {
            bytes, send, recv, ..
        } = message
        {
            (bytes, send, recv)
        } else {
            panic!("This example only supports bi-streams");
        };
        loop {
            println!(
                "Got message: {}",
                std::str::from_utf8(&bytes[..])
                    .map_err(|_| Error::Unexpected("Irregular String format".to_string()))?
            );
            println!("Enter message:");
            let input = read_from_stdin().await;
            send.send_user_msg(Bytes::from(input)).await.unwrap();
            bytes = recv.next().await?;
        }
    } else {
        println!("Echo service complete");
        let node_addr = bootstrap_nodes[0];
        let connection = endpoint.connect_to(&node_addr).await?;
        let (mut send, mut recv) = connection.open_bi_stream().await?;
        loop {
            println!("Enter message:");
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            send.send_user_msg(Bytes::from(input)).await?;
            let bytes = recv.next().await?;
            println!(
                "Got message: {}",
                std::str::from_utf8(&bytes[..])
                    .map_err(|_| Error::Unexpected("Irregular String format".to_string()))?
            );
        }
    }
}

async fn read_from_stdin() -> String {
    let mut input = String::new();
    io::stdin().read_line(&mut input).await.unwrap_or(0);
    input
}
