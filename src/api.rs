// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use super::{
    config::{Config, ConfigError, InternalConfig},
    connection_pool::ConnId,
    connections::DisconnectionEvents,
    endpoint::{Endpoint, IncomingConnections, IncomingMessages},
    error::{Error, Result},
};
use std::marker::PhantomData;
use std::net::{SocketAddr, UdpSocket};
use tracing::{debug, error, trace};

/// Main QuicP2p instance to communicate with QuicP2p using an async API
#[derive(Debug, Clone)]
pub struct QuicP2p<I: ConnId> {
    config: InternalConfig,
    phantom: PhantomData<I>,
}

impl<I: ConnId> QuicP2p<I> {
    /// Construct `QuicP2p` with supplied configuration.
    ///
    /// If `config` is `None`, the default value will be used.
    ///
    /// # Example
    ///
    /// ```
    /// use qp2p::{QuicP2p, Config, ConnId};
    /// use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    ///
    /// # #[derive(Default, Ord, PartialEq, PartialOrd, Eq, Clone, Copy)]
    /// # struct XId(pub [u8; 32]);
    /// #
    /// # impl ConnId for XId {
    /// #     fn generate(_socket_addr: &SocketAddr) -> Self {
    /// #         XId(rand::random())
    /// #     }
    /// # }
    ///
    /// let quic_p2p = QuicP2p::<XId>::with_config(Config::default())
    ///     .expect("Error initializing QuicP2p");
    /// ```
    pub fn with_config(cfg: Config) -> Result<Self, ConfigError> {
        debug!("Config passed in to qp2p: {:?}", cfg);

        Ok(Self {
            config: InternalConfig::try_from_config(cfg)?,
            phantom: PhantomData::default(),
        })
    }

    /// Bootstrap to the network.
    ///
    /// Bootstrapping will attempt to connect to all the given peers. The first successful
    /// connection to will be returned, and the others will be dropped.
    ///
    /// # Example
    ///
    /// ```
    /// use qp2p::{QuicP2p, Config, ConnId};
    /// use std::{error::Error, net::{IpAddr, Ipv4Addr, SocketAddr}};
    ///
    /// # #[derive(Default, Ord, PartialEq, PartialOrd, Eq, Clone, Copy)]
    /// # struct XId(pub [u8; 32]);
    /// #
    /// # impl ConnId for XId {
    /// #     fn generate(_socket_addr: &SocketAddr) -> Self {
    /// #         XId(rand::random())
    /// #     }
    /// # }
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn Error>> {
    ///     let local_addr = (IpAddr::V4(Ipv4Addr::LOCALHOST), 3000).into();
    ///     let quic_p2p = QuicP2p::<XId>::with_config(Config::default())?;
    ///     let (mut endpoint, _, _, _) = quic_p2p.new_endpoint(local_addr).await?;
    ///     let peer_addr = endpoint.public_addr();
    ///
    ///     let local_addr = (IpAddr::V4(Ipv4Addr::LOCALHOST), 3001).into();
    ///     let endpoint = quic_p2p.bootstrap(local_addr, vec![peer_addr]).await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn bootstrap(
        &self,
        local_addr: SocketAddr,
        bootstrap_nodes: Vec<SocketAddr>,
    ) -> Result<(
        Endpoint<I>,
        IncomingConnections,
        IncomingMessages,
        DisconnectionEvents,
        SocketAddr,
    )> {
        let (endpoint, incoming_connections, incoming_message, disconnections) =
            self.new_endpoint_with(local_addr, bootstrap_nodes).await?;

        let bootstrapped_peer = endpoint.connect_to_any(endpoint.bootstrap_nodes()).await?;

        Ok((
            endpoint,
            incoming_connections,
            incoming_message,
            disconnections,
            bootstrapped_peer,
        ))
    }

    /// Create a new [`Endpoint`] which can be used to interact with a network.
    ///
    /// `Endpoint`s can send messages to reachable peers, as well as listen to messages incoming
    /// from other peers.
    ///
    /// # Example
    ///
    /// ```
    /// use qp2p::{QuicP2p, Config, ConnId};
    /// use std::{error::Error, net::{IpAddr, Ipv4Addr, SocketAddr}};
    ///
    /// # #[derive(Default, Ord, PartialEq, PartialOrd, Eq, Clone, Copy)]
    /// # struct XId(pub [u8; 32]);
    /// #
    /// # impl ConnId for XId {
    /// #     fn generate(_socket_addr: &SocketAddr) -> Self {
    /// #         XId(rand::random())
    /// #     }
    /// # }
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn Error>> {
    ///     let local_addr = (IpAddr::V4(Ipv4Addr::LOCALHOST), 0).into();
    ///     let quic_p2p = QuicP2p::<XId>::with_config(Config::default())?;
    ///     let (endpoint, incoming_connections, incoming_messages, disconnections) = quic_p2p.new_endpoint(local_addr).await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn new_endpoint(
        &self,
        local_addr: SocketAddr,
    ) -> Result<(
        Endpoint<I>,
        IncomingConnections,
        IncomingMessages,
        DisconnectionEvents,
    )> {
        self.new_endpoint_with(local_addr, Default::default()).await
    }

    async fn new_endpoint_with(
        &self,
        local_addr: SocketAddr,
        bootstrap_nodes: Vec<SocketAddr>,
    ) -> Result<(
        Endpoint<I>,
        IncomingConnections,
        IncomingMessages,
        DisconnectionEvents,
    )> {
        trace!("Creating a new endpoint");

        let (quinn_endpoint, quinn_incoming) = bind(self.config.server.clone(), local_addr)?;

        trace!(
            "Bound endpoint to local address: {}",
            quinn_endpoint.local_addr()?
        );

        let endpoint = Endpoint::new(
            quinn_endpoint,
            quinn_incoming,
            bootstrap_nodes,
            self.config.clone(),
        )
        .await?;

        Ok(endpoint)
    }
}

// Bind a new socket with a local address
pub(crate) fn bind(
    endpoint_cfg: quinn::ServerConfig,
    local_addr: SocketAddr,
) -> Result<(quinn::Endpoint, quinn::Incoming)> {
    let mut endpoint_builder = quinn::Endpoint::builder();
    let _ = endpoint_builder.listen(endpoint_cfg);

    match UdpSocket::bind(&local_addr) {
        Ok(udp) => endpoint_builder.with_socket(udp).map_err(Error::Endpoint),
        Err(err) => {
            error!("{}", err);
            Err(Error::CannotAssignPort(local_addr.port()))
        }
    }
}
