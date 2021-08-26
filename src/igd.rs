// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use igd::SearchOptions;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::sync::broadcast::{error::TryRecvError, Receiver};
use tokio::time::{self, Instant};
use tracing::{debug, info, warn};

#[derive(Debug, thiserror::Error)]
pub(crate) enum IgdError {
    #[error("IGD is not supported for IPv6")]
    NotSupported,

    #[error(transparent)]
    AddAnyPort(#[from] igd::AddAnyPortError),

    #[error(transparent)]
    AddPort(#[from] igd::AddPortError),

    #[error(transparent)]
    Search(#[from] igd::SearchError),
}

/// Automatically forwards a port and setups a tokio task to renew it periodically.
pub(crate) async fn forward_port(
    ext_port: u16,
    local_addr: SocketAddr,
    lease_interval: Duration,
    mut termination_rx: Receiver<()>,
) -> Result<(), IgdError> {
    // Cap `lease_interval` at `u32::MAX` seconds due to limits on the IGD API. Since this is an
    // outrageous length of time (~136 years) we just do so silently.
    let lease_interval = lease_interval.min(Duration::from_secs(u32::MAX.into()));
    let lease_interval_u32 = lease_interval.as_secs() as u32;

    add_port(ext_port, local_addr, lease_interval_u32).await?;

    // Start a tokio task to renew the lease periodically.
    let _ = tokio::spawn(async move {
        let mut timer = time::interval_at(Instant::now() + lease_interval, lease_interval);

        loop {
            let _ = timer.tick().await;
            if termination_rx.try_recv() != Err(TryRecvError::Empty) {
                break;
            }
            debug!("Renewing IGD lease for port {}", local_addr);

            if let Err(error) = renew_port(ext_port, local_addr, lease_interval_u32).await {
                warn!("Failed to renew IGD lease: {} - {:?}", error, error);
            }
        }
    });

    Ok(())
}

/// Attempts to map an external port to a local address.
///
/// `local_addr` is the local listener's address that all requests will be redirected to.
/// An external port is chosen randomly and returned as a result.
///
/// `lease_duration` is the life time of a port mapping (in seconds). If it is 0, the
/// mapping will continue to exist as long as possible.
pub(crate) async fn add_port(
    ext_port: u16,
    local_addr: SocketAddr,
    lease_duration: u32,
) -> Result<(), IgdError> {
    let gateway = igd::aio::search_gateway(SearchOptions::default()).await?;

    debug!("IGD gateway found: {:?}", gateway);

    debug!("Our local address is: {:?}", local_addr);

    let local_addr = match local_addr {
        SocketAddr::V4(local_addr) => local_addr,
        _ => {
            info!("IPv6 for IGD is not supported");
            return Err(IgdError::NotSupported);
        }
    };

    gateway
        .add_port(
            igd::PortMappingProtocol::UDP,
            ext_port,
            local_addr,
            lease_duration,
            "MaidSafe.net",
        )
        .await?;

    debug!(
        "Successfully added port mapping for {} -> {}",
        ext_port, local_addr
    );

    Ok(())
}

/// Renews the lease for a specified external port.
pub(crate) async fn renew_port(
    ext_port: u16,
    local_addr: SocketAddr,
    lease_duration: u32,
) -> Result<(), IgdError> {
    let gateway = igd::aio::search_gateway(SearchOptions::default()).await?;

    if let SocketAddr::V4(socket_addr) = local_addr {
        gateway
            .add_port(
                igd::PortMappingProtocol::UDP,
                ext_port,
                socket_addr,
                lease_duration,
                "MaidSafe.net",
            )
            .await?;

        debug!("Successfully renewed the port mapping");

        Ok(())
    } else {
        info!("IPv6 for IGD is not supported");
        Err(IgdError::NotSupported)
    }
}
