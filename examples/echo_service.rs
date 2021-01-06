use anyhow::{anyhow, Error, Result};
use qp2p::{Config, QuicP2p};
use std::collections::HashSet;
use std::net::Ipv4Addr;
use std::net::{IpAddr, SocketAddr};
use structopt::StructOpt;

#[derive(Debug, StructOpt, Eq, PartialEq)]
#[structopt(rename_all = "kebab-case")]
pub struct Args {
    /// Hard Coded contacts
    /// A node will attempt to use the connection information in this list
    /// to bootstrap to the network.
    #[structopt(
        short,
        long,
        default_value = "[]",
        parse(try_from_str = serde_json::from_str)
    )]
    pub hard_coded_contacts: HashSet<SocketAddr>,
    /// Port we want to reserve for QUIC. If none supplied we'll use the OS given random port.
    /// This will be the internal port number mapped to the process
    #[structopt(short, long)]
    pub port: Option<u16>,
    /// IP address for the listener. If none supplied and `forward_port` is enabled, we will use IGD to realize the
    /// local IP address of the machine. If IGD fails the application will exit.
    #[structopt(long)]
    pub ip: Option<IpAddr>,
    /// Specify if port forwarding via UPnP should be done or not. This can be set to false if the network
    /// is run locally on the network loopback or on a local area network.
    #[structopt(long)]
    pub forward_port: bool,
    /// Is the network meant to run on the local loopback network?
    /// If this is set to true, the IP address will be set to `127.0.0.1`
    #[structopt(long)]
    pub local: bool,
    /// Set to true if this is the first or genesis node
    #[structopt(long)]
    pub first: bool,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let mut args = Args::from_args();

    if args.local {
        args.ip = Some(IpAddr::V4(Ipv4Addr::LOCALHOST));
        args.forward_port = false;
    }

    let qp2p = QuicP2p::with_config(
        Some(Config {
            ip: args.ip,
            port: args.port,
            forward_port: args.forward_port,
            hard_coded_contacts: args.hard_coded_contacts,
            ..Default::default()
        }),
        &[],
        false,
    )?;

    let mut endpoint = qp2p.new_endpoint()?;
    let local_addr = endpoint.local_addr();
    
    let public_addr = if args.forward_port {
        endpoint.public_addr().await?
    } else {
        local_addr
    };

    println!(
        "Node started.\nLocal adddress: {}\nPublic Address: {}",
        local_addr,
        public_addr        
    );

    loop {
        println!("Waiting for connections...\n");
        let mut incoming = endpoint.listen();
        let mut messages = incoming
            .next()
            .await
            .ok_or_else(|| anyhow!("Missing expected incoming connection"))?;
        let connecting_peer = messages.remote_addr();
        println!("Incoming connection from: {}", &connecting_peer);

        let _message = messages
            .next()
            .await
            .ok_or_else(|| anyhow!("Missing expected incomming message"))?;
        println!("Responded to peer with EchoService response\n");
    }
}
