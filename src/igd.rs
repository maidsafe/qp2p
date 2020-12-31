// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::error::{Error, Result};
use log::{debug, info, warn};
use std::net::{IpAddr, SocketAddr, SocketAddrV4};
use std::time::Duration;
use tokio::time::{self, Instant};

/// Automatically forwards a port and setups a tokio task to renew it periodically.
pub async fn forward_port(local_addr: SocketAddr, lease_duration: u32) -> Result<SocketAddrV4> {
    let igd_res = add_port(local_addr, lease_duration).await;

    if let Ok(ref ext_sock_addr) = igd_res {
        // Start a tokio task to renew the lease periodically.
        let ext_port = ext_sock_addr.port();
        let lease_interval = Duration::from_secs(lease_duration.into());

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
pub(crate) async fn add_port(local_addr: SocketAddr, lease_duration: u32) -> Result<SocketAddrV4> {
    let gateway = igd::aio::search_gateway(Default::default()).await?;

    debug!("IGD gateway found: {:?}", gateway);

    debug!("Our local address is: {:?}", local_addr);

    if let SocketAddr::V4(socket_addr) = local_addr {
        let ext_addr = gateway
            .get_any_address(
                igd::PortMappingProtocol::UDP,
                socket_addr,
                lease_duration,
                "MaidSafe.net",
            )
            .await?;

        debug!("Our external port number is: {}", ext_addr.port());

        Ok(ext_addr)
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
    let gateway = igd::aio::search_gateway(Default::default()).await?;

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

// Find our local IP address by connecting to the gateway and querying local socket address.
pub(crate) fn get_local_ip() -> Result<IpAddr> {
    debug!("Attempting to realise local IP address with IGD...");
    let gateway = igd::search_gateway(Default::default())?;
    let gateway_conn = std::net::TcpStream::connect(gateway.addr)?;
    let local_sa = gateway_conn.local_addr()?;
    info!("Fetched IP address from IGD gateway: {:?}", &local_sa.ip());
    Ok(local_sa.ip())
}
