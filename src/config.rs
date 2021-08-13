// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

//! Configuration for `Endpoint`s.

use crate::{
    error::{Error, Result},
    utils,
};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::{fmt, net::IpAddr, str::FromStr, sync::Arc, time::Duration};
use structopt::StructOpt;

/// Default for [`Config::idle_timeout`] (1 minute).
///
/// This is based on average time in which routers would close the UDP mapping to the peer if they
/// see no conversation between them.
pub const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// Default for [`Config::keep_alive_interval`] (20 seconds).
pub const DEFAULT_KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(20);

/// Default for [`Config::upnp_lease_duration`] (2 minutes).
pub const DEFAULT_UPNP_LEASE_DURATION: Duration = Duration::from_secs(120);

/// Default for [`Config::min_retry_duration`] (30 seconds).
pub const DEFAULT_MIN_RETRY_DURATION: Duration = Duration::from_secs(30);

const MAIDSAFE_DOMAIN: &str = "maidsafe.net";

/// QuicP2p configurations
#[derive(Clone, Debug, Default, Serialize, Deserialize, Eq, PartialEq, StructOpt)]
pub struct Config {
    /// Specify if port forwarding via UPnP should be done or not. This can be set to false if the network
    /// is run locally on the network loopback or on a local area network.
    #[structopt(long)]
    pub forward_port: bool,

    /// External port number assigned to the socket address of the program.
    /// If this is provided, QP2p considers that the local port provided has been mapped to the
    /// provided external port number and automatic port forwarding will be skipped.
    #[structopt(long)]
    pub external_port: Option<u16>,

    /// External IP address of the computer on the WAN. This field is mandatory if the node is the genesis node and
    /// port forwarding is not available. In case of non-genesis nodes, the external IP address will be resolved
    /// using the Echo service.
    #[structopt(long)]
    pub external_ip: Option<IpAddr>,

    /// How long to wait to hear from a peer before timing out a connection.
    ///
    /// In the absence of any keep-alive messages, connections will be closed if they remain idle
    /// for at least this duration.
    ///
    /// If unspecified, this will default to [`DEFAULT_IDLE_TIMEOUT`].
    #[serde(default)]
    #[structopt(long, parse(try_from_str = parse_millis), value_name = "MILLIS")]
    pub idle_timeout: Option<Duration>,

    /// Interval at which to send keep-alives to maintain otherwise idle connections.
    ///
    /// Keep-alives prevent otherwise idle connections from timing out.
    ///
    /// If unspecified, this will default to [`DEFAULT_KEEP_ALIVE_INTERVAL`].
    #[serde(default)]
    #[structopt(long, parse(try_from_str = parse_millis), value_name = "MILLIS")]
    pub keep_alive_interval: Option<Duration>,

    /// How long UPnP port mappings will last.
    ///
    /// Note that UPnP port mappings will be automatically renewed on this interval.
    ///
    /// If unspecified, this will default to [`DEFAULT_UPNP_LEASE_DURATION`], which should be
    /// suitable in most cases but some routers may clear UPnP port mapping more frequently.
    #[serde(default)]
    #[structopt(long, parse(try_from_str = parse_millis), value_name = "MILLIS")]
    pub upnp_lease_duration: Option<Duration>,

    /// How long to retry establishing connections and sending messages.
    ///
    /// Retrying will continue for *at least* this duration, but potentially longer as an
    /// in-progress back-off delay will not be interrupted.
    ///
    /// If unspecified, this will default to [`DEFAULT_MIN_RETRY_DURATION`].
    #[serde(default)]
    #[structopt(long, parse(try_from_str = parse_millis), value_name = "MILLIS")]
    pub min_retry_duration: Option<Duration>,
}

fn parse_millis(millis: &str) -> Result<Duration, std::num::ParseIntError> {
    Ok(Duration::from_millis(millis.parse()?))
}

/// Config that has passed validation.
///
/// Generally this is a copy of [`Config`] without optional values where we would use defaults.
#[derive(Clone, Debug)]
pub(crate) struct InternalConfig {
    pub(crate) client: quinn::ClientConfig,
    pub(crate) server: quinn::ServerConfig,
    pub(crate) forward_port: bool,
    pub(crate) external_port: Option<u16>,
    pub(crate) external_ip: Option<IpAddr>,
    pub(crate) upnp_lease_duration: Duration,
    pub(crate) min_retry_duration: Duration,
}

impl InternalConfig {
    pub(crate) fn try_from_config(config: Config) -> Result<Self> {
        let idle_timeout = config.idle_timeout.unwrap_or(DEFAULT_IDLE_TIMEOUT);
        let keep_alive_interval = config
            .keep_alive_interval
            .unwrap_or(DEFAULT_KEEP_ALIVE_INTERVAL);
        let upnp_lease_duration = config
            .upnp_lease_duration
            .unwrap_or(DEFAULT_UPNP_LEASE_DURATION);
        let min_retry_duration = config
            .min_retry_duration
            .unwrap_or(DEFAULT_MIN_RETRY_DURATION);

        let transport = Self::new_transport_config(idle_timeout, keep_alive_interval);
        let client = Self::new_client_config(transport.clone());
        let server = Self::new_server_config(transport)?;

        Ok(Self {
            client,
            server,
            forward_port: config.forward_port,
            external_port: config.external_port,
            external_ip: config.external_ip,
            upnp_lease_duration,
            min_retry_duration,
        })
    }

    fn new_transport_config(
        idle_timeout: Duration,
        keep_alive_interval: Duration,
    ) -> Arc<quinn::TransportConfig> {
        let mut config = quinn::TransportConfig::default();

        // QUIC encodes idle timeout in a varint with max size 2^62, which is below what can be
        // represented by Duration. For now, just ignore too large idle timeouts.
        // FIXME: don't ignore (e.g. clamp/error/panic)?
        let _ = config.max_idle_timeout(Some(idle_timeout)).ok();
        let _ = config.keep_alive_interval(Some(keep_alive_interval));

        Arc::new(config)
    }

    fn new_client_config(transport: Arc<quinn::TransportConfig>) -> quinn::ClientConfig {
        let mut config = quinn::ClientConfig {
            transport,
            ..Default::default()
        };
        Arc::make_mut(&mut config.crypto)
            .dangerous()
            .set_certificate_verifier(Arc::new(SkipCertificateVerification));
        config
    }

    fn new_server_config(transport: Arc<quinn::TransportConfig>) -> Result<quinn::ServerConfig> {
        let (key, cert) = SerialisableCertificate::new(vec![MAIDSAFE_DOMAIN.to_string()])?
            .obtain_priv_key_and_cert()?;

        let mut config = quinn::ServerConfig::default();
        config.transport = transport;

        let mut config = quinn::ServerConfigBuilder::new(config);
        let _ = config.certificate(quinn::CertificateChain::from_certs(vec![cert]), key)?;

        Ok(config.build())
    }
}

struct SkipCertificateVerification;

impl rustls::ServerCertVerifier for SkipCertificateVerification {
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

/// To be used to read and write our certificate and private key to disk esp. as a part of our
/// configuration file
#[derive(Serialize, Deserialize, Clone, Eq, PartialEq)]
struct SerialisableCertificate {
    /// DER encoded certificate
    cert_der: Bytes,
    /// DER encoded private key
    key_der: Bytes,
}

impl SerialisableCertificate {
    /// Returns a new Certificate that is valid for the list of domain names provided
    fn new(domains: impl Into<Vec<String>>) -> Result<Self> {
        let cert = rcgen::generate_simple_self_signed(domains)?;
        Ok(Self {
            cert_der: cert.serialize_der()?.into(),
            key_der: cert.serialize_private_key_der().into(),
        })
    }

    /// Parses DER encoded binary key material to a format that can be used by Quinn
    ///
    /// # Errors
    /// Returns [CertificateParseError](Error::CertificateParseError) if the inputs
    /// cannot be parsed
    fn obtain_priv_key_and_cert(&self) -> Result<(quinn::PrivateKey, quinn::Certificate)> {
        Ok((
            quinn::PrivateKey::from_der(&self.key_der).map_err(|_| Error::CertificatePkParse)?,
            quinn::Certificate::from_der(&self.cert_der).map_err(|_| Error::CertificateParse)?,
        ))
    }
}

impl FromStr for SerialisableCertificate {
    type Err = Error;

    /// Decode `SerialisableCertificate` from a base64-encoded string.
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let cert = base64::decode(s)?;
        let (cert_der, key_der) = bincode::deserialize(&cert)?;
        Ok(Self { cert_der, key_der })
    }
}

impl fmt::Debug for SerialisableCertificate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "SerialisableCertificate {{ cert_der: {}, key_der: <HIDDEN> }}",
            utils::bin_data_format(&self.cert_der)
        )
    }
}
