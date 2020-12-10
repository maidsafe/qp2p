use bytes::Bytes;
use qp2p::{Config, Error, Message, QuicP2p};
use std::env;
use tokio::io::AsyncBufReadExt;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let args: Vec<String> = env::args().collect();
    let (bootstrap_nodes, genesis) = match &args[1][..] {
        "create" => (vec![], true),
        "connect" => {
            let bootstrap_node = args[2].parse().map_err(|_| {
                Error::Configuration("SocketAddr format not recognized".to_string())
            })?;
            (vec![bootstrap_node], false)
        }
        other => {
            return Err(Error::Configuration(format!(
                "Unexpected argument: {}",
                other
            )));
        }
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
    let endpoint = qp2p.new_endpoint()?;
    let socket_addr = endpoint.socket_addr().await?;
    println!("Process running at: {}", &socket_addr);
    if genesis {
        println!("Waiting for connections");
        let mut incoming = endpoint.listen();
        let mut messages = incoming.next().await.ok_or(Error::NoIncomingConnection)?;
        let connecting_peer = messages.remote_addr();
        println!("Incoming connection from: {}", &connecting_peer);
        let message = messages.next().await.ok_or(Error::NoIncomingMessage)?;
        println!("Responded to peer with EchoService response");
        println!("Waiting for messages...");
        let (mut bytes, mut send, mut recv) = if let Message::BiStream {
            bytes, send, recv, ..
        } = message
        {
            (bytes, send, recv)
        } else {
            println!("Only bidirectional streams are supported in this example");
            return Err(Error::OperationNotAllowed);
        };
        loop {
            println!(
                "Got message: {}",
                std::str::from_utf8(&bytes[..])
                    .map_err(|_| Error::Unexpected("Irregular String format".to_string()))?
            );
            println!("Enter message:");
            let input = read_from_stdin().await;
            send.send_user_msg(Bytes::from(input)).await?;
            bytes = recv.next().await?;
        }
    } else {
        println!("Echo service complete");
        let node_addr = bootstrap_nodes[0];
        let (connection, _) = endpoint.connect_to(&node_addr).await?;
        let (mut send, mut recv) = connection.open_bi().await?;
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
    let stdin = tokio::io::stdin();
    let mut buf_reader = tokio::io::BufReader::new(stdin);
    buf_reader.read_line(&mut input).await.unwrap_or(0);
    input
}
