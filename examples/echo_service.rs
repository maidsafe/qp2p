use std::env;
use futures::select;
use qp2p::{QuicP2p, Config, Error};
use async_std::io;

#[tokio::main]
async fn main() -> Result<(), Error>{
    let args: Vec<String> = env::args().collect();
    let mut connect = false;
    let (bootstrap_nodes, genesis) = match &args[1][..] {
        "create" => {
            (vec![], true)
        }
        "connect" => {
            let bootstrap_node = args[2].parse().expect("SocketAddr format not recognized");
            (vec![bootstrap_node], false)
        }
        _ => panic!("Unexpected argument")
    };
    let qp2p = QuicP2p::with_config(Some(Config {
        ip: None,
        port: Some(0),
        ..Default::default()
    }), bootstrap_nodes.into(), false)?;
    let mut endpoint = qp2p.new_endpoint()?;
    let socket_addr = endpoint.our_endpoint().await?;
    println!("Process running at: {}", &socket_addr);
    if genesis {
        println!("Waiting for connections");
        let mut incoming = endpoint.listen()?;
        let mut messages = incoming.next().await.ok_or(Error::Unexpected("Error during incoming connection".to_string()))?;
        let connecting_peer = messages.remote_addr();
        println!("Incoming connection from: {}", &connecting_peer);
        let message = messages.next().await;
        assert!(message.is_none());
        println!("Responded to peer with EchoService response");
        Ok(())
    } else {
        println!("Echo service complete");
        Ok(())
    }
}