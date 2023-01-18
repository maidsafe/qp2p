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
    let (node, mut incoming_conns) = Endpoint::new_peer(
        SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
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
            let (conn, mut incoming) = node.connect_to(&peer).await?;
            conn.send((Bytes::new(), Bytes::new(), msg.clone())).await?;
            // `Endpoint` no longer having `connection_pool` to hold established connection.
            // Which means the connection get closed immediately when it reaches end of life span.
            // And causes the receiver side a sending error when reply via the in-coming connection.
            // Hence here have to listen for the reply to avoid such error
            let reply = incoming.next().await.unwrap();
            println!("Received from {:?} --> {:?}", peer, reply);
        }

        println!("Done sending");
    }

    println!("\n---");
    println!("Listening on: {:?}", node.local_addr());
    println!("---\n");

    // loop over incoming connections
    while let Some((connection, mut incoming)) = incoming_conns.next().await {
        let src = connection.remote_address();

        // loop over incoming messages
        while let Ok(Some((_, _, bytes))) = incoming.next().await {
            println!("Received from {:?} --> {:?}", src, bytes);
            if bytes == *MSG_MARCO {
                let reply = Bytes::from(MSG_POLO);
                connection
                    .send((Bytes::new(), Bytes::new(), reply.clone()))
                    .await?;
                println!("Replied to {:?} --> {:?}", src, reply);
            }
            println!();
        }
    }

    Ok(())
}
