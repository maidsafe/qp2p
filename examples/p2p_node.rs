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

use bytes::Bytes;
use color_eyre::eyre::Result;
use qp2p::{Config, Endpoint};
use std::{
    env,
    net::{Ipv4Addr, SocketAddr},
    time::Duration,
};

#[derive(Default, Ord, PartialEq, PartialOrd, Eq, Clone, Copy)]
struct XId(pub [u8; 32]);

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    const MSG_MARCO: &str = "marco";
    const MSG_POLO: &str = "polo";

    // collect cli args
    let args: Vec<String> = env::args().collect();

    // create an endpoint for us to listen on and send from.
    let (node, _incoming_conns, mut incoming_messages, _disconnections, _contact) = Endpoint::new(
        SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        &[],
        Config {
            idle_timeout: Duration::from_secs(60 * 60).into(), // 1 hour idle timeout.
            ..Default::default()
        },
    )
    .await?;

    // if we received args then we parse them as SocketAddr and send a "marco" msg to each peer.
    if args.len() > 1 {
        for arg in args.iter().skip(1) {
            let peer: SocketAddr = arg
                .parse()
                .expect("Invalid SocketAddr.  Use the form 127.0.0.1:1234");
            let msg = Bytes::from(MSG_MARCO);
            println!("Sending to {:?} --> {:?}\n", peer, msg);
            let conn = node.connect_to(&peer).await?;
            conn.send_uni(msg.clone(), 0).await?;
        }
    }

    println!("\n---");
    println!("Listening on: {:?}", node.public_addr());
    println!("---\n");

    // loop over incoming messages
    while let Some((src, bytes)) = incoming_messages.next().await {
        println!("Received from {:?} --> {:?}", src.remote_address(), bytes);
        if bytes == *MSG_MARCO {
            let reply = Bytes::from(MSG_POLO);
            src.send_uni(reply.clone(), 0).await?;
            println!("Replied to {:?} --> {:?}", src.remote_address(), reply);
        }
        println!();
    }

    Ok(())
}
