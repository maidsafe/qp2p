// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::error::QuicP2pError;
use crate::utils::R;
use log::{debug, info, warn};
use std::net::{IpAddr, SocketAddrV4};
use std::time::Duration;
use tokio::time::{self, Instant};

/// Default duration of a UPnP lease, in seconds.
pub const DEFAULT_UPNP_LEASE_DURATION_SEC: u32 = 120;
/// Duration of wait for a UPnP result to come back.
pub const UPNP_RESPONSE_TIMEOUT_MSEC: u64 = 1_000;

/// Automatically forwards a port and setups a tokio task to renew it periodically.
pub async fn forward_port(local_port: u16, lease_duration: u32) -> R<SocketAddrV4> {
    let igd_res = add_port(local_port, lease_duration).await;

    if let Ok(ref ext_sock_addr) = igd_res {
        // Start a tokio task to renew the lease periodically.
        let ext_port = ext_sock_addr.port();
        let lease_interval = Duration::from_secs(lease_duration.into());

        let _ = tokio::spawn(async move {
            let mut timer =
                time::interval_at(Instant::now() + lease_interval.into(), lease_interval);

            loop {
                let _ = timer.tick().await;
                debug!("Renewing IGD lease for port {}", local_port);

                let renew_res = renew_port(local_port, ext_port, lease_duration).await;
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
/// `local_port` is the local listener's port that all requests will be redirected to.
/// An external port is chosen randomly and returned as a result.
///
/// `lease_duration` is the life time of a port mapping (in seconds). If it is 0, the
/// mapping will continue to exist as long as possible.
pub(crate) async fn add_port(local_port: u16, lease_duration: u32) -> R<SocketAddrV4> {
    let gateway = igd::aio::search_gateway(Default::default()).await?;

    debug!("Found IGD gateway: {:?}", gateway);

    // Find our local IP address by connecting to the gateway and querying local socket address.
    let gateway_conn = tokio::net::TcpStream::connect(gateway.addr).await?;
    let local_sa = gateway_conn.local_addr()?;

    if let IpAddr::V4(ip) = local_sa.ip() {
        let local_sa = SocketAddrV4::new(ip, local_port);

        let ext_addr = gateway
            .get_any_address(
                igd::PortMappingProtocol::UDP,
                local_sa,
                lease_duration,
                "MaidSafe.net",
            )
            .await?;

        debug!("Our external address is {:?}", ext_addr);

        Ok(ext_addr)
    } else {
        info!("IPv6 for IGD is not supported");
        Err(QuicP2pError::IgdNotSupported)
    }
}

/// Renews the lease for a specified external port.
pub(crate) async fn renew_port(local_port: u16, ext_port: u16, lease_duration: u32) -> R<()> {
    let gateway = igd::aio::search_gateway(Default::default()).await?;

    // Find our local IP address by connecting to the gateway and querying local socket address.
    let gateway_conn = tokio::net::TcpStream::connect(gateway.addr).await?;
    let local_sa = gateway_conn.local_addr()?;

    if let IpAddr::V4(ip) = local_sa.ip() {
        let local_sa = SocketAddrV4::new(ip, local_port);

        let _res = gateway
            .add_port(
                igd::PortMappingProtocol::UDP,
                ext_port,
                local_sa,
                lease_duration,
                "MaidSafe.net",
            )
            .await;

        debug!("Successfully renewed the port mapping");

        Ok(())
    } else {
        info!("IPv6 for IGD is not supported");
        Err(QuicP2pError::IgdNotSupported)
    }
}
