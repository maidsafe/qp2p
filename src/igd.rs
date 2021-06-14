// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::error::{Error, Result};
use igd::SearchOptions;
use log::{debug, info, warn};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::time::{self, Instant};

/// Automatically forwards a port and setups a tokio task to renew it periodically.
pub async fn forward_port(local_addr: SocketAddr, lease_duration: u32) -> Result<u16> {
    let igd_res = add_port(local_addr, lease_duration).await;

    if let Ok(ext_port) = &igd_res {
        // Start a tokio task to renew the lease periodically.
        let lease_interval = Duration::from_secs(lease_duration.into());
        let ext_port = *ext_port;
        let _ = tokio::spawn(async move {
            let mut timer = time::interval_at(Instant::now() + lease_interval, lease_interval);

            loop {
                let _ = timer.tick().await;
                debug!("Renewing IGD lease for port {}", local_addr);

                let renew_res = renew_port(local_addr, ext_port, lease_duration).await;
                match renew_res {
                    Ok(()) => {}
                    Err(e) => {
                        warn!("Failed to renew IGD lease: {} - {:?}", e, e);
                    }
                }
            }
        });
    }

    igd_res
}

/// Attempts to map an external port to a local address.
///
/// `local_addr` is the local listener's address that all requests will be redirected to.
/// An external port is chosen randomly and returned as a result.
///
/// `lease_duration` is the life time of a port mapping (in seconds). If it is 0, the
/// mapping will continue to exist as long as possible.
pub(crate) async fn add_port(local_addr: SocketAddr, lease_duration: u32) -> Result<u16> {
    let gateway = igd::aio::search_gateway(SearchOptions::default()).await?;

    debug!("IGD gateway found: {:?}", gateway);

    debug!("Our local address is: {:?}", local_addr);

    if let SocketAddr::V4(socket_addr) = local_addr {
        gateway
            .add_port(
                igd::PortMappingProtocol::UDP,
                socket_addr.port(),
                socket_addr,
                lease_duration,
                "MaidSafe.net",
            )
            .await?;

        let ext_port = socket_addr.port();
        debug!("Our external port number is: {}", socket_addr.port());

        Ok(ext_port)
    } else {
        info!("IPv6 for IGD is not supported");
        Err(Error::IgdNotSupported)
    }
}

/// Renews the lease for a specified external port.
pub(crate) async fn renew_port(
    local_addr: SocketAddr,
    ext_port: u16,
    lease_duration: u32,
) -> Result<()> {
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
        Err(Error::IgdNotSupported)
    }
}
