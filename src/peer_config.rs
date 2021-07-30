// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::Result;
use std::{sync::Arc, time::Duration};

/// Default interval within which if we hear nothing from the peer we declare it offline to us.
///
/// This is based on average time in which routers would close the UDP mapping to the peer if they
/// see no conversation between them.
///
/// The value is in milliseconds.
pub const DEFAULT_IDLE_TIMEOUT_MSEC: u64 = 60_000; // 60secs
/// Default Interval to send keep-alives if we are idling so that the peer does not disconnect from
/// us declaring us offline. If none is supplied we'll default to the documented constant.
///
/// The value is in milliseconds.
pub const DEFAULT_KEEP_ALIVE_INTERVAL_MSEC: u32 = 20_000; // 20secs

pub fn new_client_cfg(
    idle_timeout_msec: u64,
    keep_alive_interval_msec: u32,
) -> Result<quinn::ClientConfig> {
    let mut config = quinn::ClientConfig::default();
    let crypto_cfg = Arc::make_mut(&mut config.crypto);
    crypto_cfg
        .dangerous()
        .set_certificate_verifier(SkipServerVerification::new());
    config.transport = Arc::new(new_transport_cfg(
        idle_timeout_msec,
        keep_alive_interval_msec,
    ));
    Ok(config)
}

pub fn new_our_cfg(
    idle_timeout_msec: u64,
    keep_alive_interval_msec: u32,
    our_cert: quinn::Certificate,
    our_key: quinn::PrivateKey,
) -> Result<quinn::ServerConfig> {
    let mut our_cfg_builder = {
        let mut our_cfg = quinn::ServerConfig::default();
        our_cfg.transport = Arc::new(new_transport_cfg(
            idle_timeout_msec,
            keep_alive_interval_msec,
        ));

        quinn::ServerConfigBuilder::new(our_cfg)
    };
    let _ = our_cfg_builder
        .certificate(quinn::CertificateChain::from_certs(vec![our_cert]), our_key)?;

    Ok(our_cfg_builder.build())
}

fn new_transport_cfg(
    idle_timeout_msec: u64,
    keep_alive_interval_msec: u32,
) -> quinn::TransportConfig {
    let mut transport_config = quinn::TransportConfig::default();
    let _ = transport_config
        .max_idle_timeout(Some(Duration::from_millis(idle_timeout_msec)))
        .unwrap_or(&mut quinn::TransportConfig::default());
    let _ = transport_config
        .keep_alive_interval(Some(Duration::from_millis(keep_alive_interval_msec.into())));
    transport_config
}

/// Dummy certificate verifier that treats any certificate as valid.
struct SkipServerVerification;

impl SkipServerVerification {
    fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

impl rustls::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _roots: &rustls::RootCertStore,
        _presented_certs: &[rustls::Certificate],
        _dns_name: webpki::DNSNameRef,
        _ocsp_response: &[u8],
    ) -> std::result::Result<rustls::ServerCertVerified, rustls::TLSError> {
        Ok(rustls::ServerCertVerified::assertion())
    }
}
