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

pub(crate) fn new_client_cfg(
    idle_timeout: Duration,
    keep_alive_interval: Duration,
) -> Result<quinn::ClientConfig> {
    let mut config = quinn::ClientConfig::default();
    let crypto_cfg = Arc::make_mut(&mut config.crypto);
    crypto_cfg
        .dangerous()
        .set_certificate_verifier(SkipServerVerification::new());
    config.transport = Arc::new(new_transport_cfg(idle_timeout, keep_alive_interval));
    Ok(config)
}

pub(crate) fn new_our_cfg(
    idle_timeout: Duration,
    keep_alive_interval: Duration,
    our_cert: quinn::Certificate,
    our_key: quinn::PrivateKey,
) -> Result<quinn::ServerConfig> {
    let mut our_cfg_builder = {
        let mut our_cfg = quinn::ServerConfig::default();
        our_cfg.transport = Arc::new(new_transport_cfg(idle_timeout, keep_alive_interval));

        quinn::ServerConfigBuilder::new(our_cfg)
    };
    let _ = our_cfg_builder
        .certificate(quinn::CertificateChain::from_certs(vec![our_cert]), our_key)?;

    Ok(our_cfg_builder.build())
}

fn new_transport_cfg(
    idle_timeout: Duration,
    keep_alive_interval: Duration,
) -> quinn::TransportConfig {
    let mut transport_config = quinn::TransportConfig::default();
    let _ = transport_config
        .max_idle_timeout(Some(idle_timeout))
        .unwrap_or(&mut quinn::TransportConfig::default());
    let _ = transport_config.keep_alive_interval(Some(keep_alive_interval));
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
