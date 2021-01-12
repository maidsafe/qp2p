use anyhow::{anyhow, Error, Result};
use qp2p::{Config, QuicP2p, Error as QP2pError};
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
    /// If external port is provided it means that the user is carrying out manual port forwarding and this field is mandatory.
    /// This will be the internal port number mapped to the process
    #[structopt(short, long)]
    pub local_port: Option<u16>,
    /// IP address for the listener. If none is supplied and `forward_port` is enabled, we will use IGD to realize the
    /// local IP address of the machine. If IGD fails the application will exit.
    #[structopt(short, long)]
    pub local_ip: Option<IpAddr>,
    /// Specify if port forwarding via UPnP should be done or not. This can be set to false if the network
    /// is run locally on the network loopback or on a local area network.
    #[structopt(long)]
    pub forward_port: bool,
    /// External port number assigned to the socket address of the program. 
    /// If this is provided, QP2p considers that the local port provided has been mapped to the
    /// provided external port number and automatic port forwarding will be skipped.
    #[structopt(short, long)]
    pub external_port: Option<u16>,
    /// External IP address of the computer on the WAN. This field is mandatory if the node is the genesis node and 
    /// port forwarding is not available. In case of non-genesis nodes, the external IP address will be resolved
    /// using the Echo service.
    #[structopt(short, long)]
    pub external_ip: Option<IpAddr>,
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
        args.local_ip = Some(IpAddr::V4(Ipv4Addr::LOCALHOST));
        args.forward_port = false;
    }

    let qp2p_res = QuicP2p::with_config(
        Some(Config {
            local_ip: args.local_ip,
            local_port: args.local_port,
            external_ip: args.external_ip,
            external_port: args.external_port,
            forward_port: args.forward_port,
            hard_coded_contacts: args.hard_coded_contacts,
            ..Default::default()
        }),
        &[],
        false,
    );

    let qp2p = match qp2p_res {
        Ok(qp2p) => qp2p,
        Err(err) => {
            if let QP2pError::IgdSearch(_) = err { 
                println!("The program encountered an error while automatically trying to realize local IP Address.");
                println!("This is because IGD is not supported by your router.");
                println!("Please restart the program by manually forwarding the port and providing the required information by passing --local-ip x.x.x.x --external-ip x.x.x.x --local-port xxxx --external-port xxxx");
            };
            return Err(Error::from(err));
        }
    };

    let mut endpoint = qp2p.new_endpoint().await?;
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

    // Message is declared here and later assigned so that it is held in memory
    // long enough for the peer to read the Echo Service responses before it is
    // dropped and the streams are closed
    #[allow(unused)]
    let mut message: qp2p::Message;
    
    #[allow(unused)]
    loop {
        println!("Waiting for connections...\n");
        let mut incoming = endpoint.listen();
        let mut messages = incoming
            .next()
            .await
            .ok_or_else(|| anyhow!("Missing expected incoming connection"))?;
        let connecting_peer = messages.remote_addr();
        println!("Incoming connection from: {}", &connecting_peer);

        message = messages
            .next()
            .await
            .ok_or_else(|| anyhow!("Missing expected incomming message"))?;
        println!("Responded to peer with EchoService response\n");
    }
}
