// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::context::ctx;
use crate::R;
use std::sync::Arc;

/// Default interval within which if we hear nothing from the peer we declare it offline to us.
///
/// This is based on average time in which routers would close the UDP mapping to the peer if they
/// see no conversation between them.
///
/// The value is in milliseconds.
pub const DEFAULT_IDLE_TIMEOUT_MSEC: u64 = 30_000; // 30secs
/// Default Interval to send keep-alives if we are idling so that the peer does not disconnect from
/// us declaring us offline. If none is supplied we'll default to the documented constant.
///
/// The value is in milliseconds.
pub const DEFAULT_KEEP_ALIVE_INTERVAL_MSEC: u32 = 10_000; // 10secs

pub fn new_client_cfg() -> quinn::ClientConfig {
    let mut cfg = quinn::ClientConfigBuilder::default().build();
    let crypto_cfg =
        Arc::get_mut(&mut cfg.crypto).expect("the crypto config should not be shared yet");
    crypto_cfg
        .dangerous()
        .set_certificate_verifier(SkipServerVerification::new());
    cfg.transport = Arc::new(new_transport_cfg(None, None));
    cfg
}

pub fn new_our_cfg(
    idle_timeout_msec: u64,
    keep_alive_interval_msec: u32,
    our_cert: quinn::Certificate,
    our_key: quinn::PrivateKey,
) -> R<quinn::ServerConfig> {
    let mut our_cfg_builder = {
        let mut our_cfg = quinn::ServerConfig::default();
        our_cfg.transport = Arc::new(new_transport_cfg(
            Some(idle_timeout_msec),
            Some(keep_alive_interval_msec),
        ));

        quinn::ServerConfigBuilder::new(our_cfg)
    };
    let _ = our_cfg_builder
        .certificate(quinn::CertificateChain::from_certs(vec![our_cert]), our_key)?
        .use_stateless_retry(true);

    Ok(our_cfg_builder.build())
}

fn new_transport_cfg(
    idle_timeout_msec: Option<u64>,
    keep_alive_interval_msec: Option<u32>,
) -> quinn::TransportConfig {
    let mut transport_cfg = quinn::TransportConfig::default();
    transport_cfg.idle_timeout = idle_timeout_msec.unwrap_or_else(|| ctx(|c| c.idle_timeout_msec));
    transport_cfg.keep_alive_interval =
        keep_alive_interval_msec.unwrap_or_else(|| ctx(|c| c.keep_alive_interval_msec));

    transport_cfg
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
    ) -> Result<rustls::ServerCertVerified, rustls::TLSError> {
        Ok(rustls::ServerCertVerified::assertion())
    }
}
