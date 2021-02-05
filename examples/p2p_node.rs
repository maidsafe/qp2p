//! This example demonstrates accepting connections and messages
//! on a socket/port and replying on the same socket/port using a
//! bidirectional stream.
//!
//! We implement a simple P2P node that listens for incoming messages
//! from an arbitrary number of peers. If a peer sends us "marco" we reply
//! with "polo".
//!
//! Our node accepts a list of SocketAddr for peers on the command-line.
//! Upon startup, we send "marco" to each peer in the list and print
//! the reply.  If the list is empty, we don't send any message.
//!
//! We then proceed to listening for new connections/messages.

use anyhow::Result;
use bytes::Bytes;
use qp2p::{Config, QuicP2p};
use std::env;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

#[tokio::main]
async fn main() -> Result<()> {
    const MSG_MARCO: &str = "marco";
    const MSG_POLO: &str = "polo";

    // collect cli args
    let args: Vec<String> = env::args().collect();

    // instantiate QuicP2p with custom config
    let qp2p = QuicP2p::with_config(
        Some(Config {
            local_ip: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
            idle_timeout_msec: Some(1000 * 3600), // 1 hour idle timeout.
            ..Default::default()
        }),
        Default::default(),
        true,
    )?;

    // create an endpoint for us to listen on and send from.
    let (node, _incoming_conns, mut incoming_messages, _disconnections) =
        qp2p.new_endpoint().await?;

    // if we received args then we parse them as SocketAddr and send a "marco" msg to each peer.
    if args.len() > 1 {
        for arg in args.iter().skip(1) {
            let peer: SocketAddr = arg
                .parse()
                .expect("Invalid SocketAddr.  Use the form 127.0.0.1:1234");
            let msg = Bytes::from(MSG_MARCO);
            println!("Sending to {:?} --> {:?}\n", peer, msg);
            node.connect_to(&peer).await?;
            node.send_message(msg.clone(), &peer).await?;
        }
    }

    println!("\n---");
    println!("Listening on: {:?}", node.socket_addr());
    println!("---\n");

    // loop over incoming messages
    while let Some((socket_addr, bytes)) = incoming_messages.next().await {
        println!("Received from {:?} --> {:?}", socket_addr, bytes);
        if bytes == Bytes::from(MSG_MARCO) {
            let reply = Bytes::from(MSG_POLO);
            node.send_message(reply.clone(), &socket_addr).await?;
            println!("Replied to {:?} --> {:?}", socket_addr, reply);
        }
        println!();
    }

    Ok(())
}
