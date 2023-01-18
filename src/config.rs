// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

//! Configuration for `Endpoint`s.

use quinn::{IdleTimeout, VarInt};

use rustls::{Certificate, ClientConfig, ServerName};
use serde::{Deserialize, Serialize};
use std::{net::IpAddr, sync::Arc, time::Duration};

#[cfg(feature = "structopt")]
use structopt::StructOpt;

/// Default for [`Config::idle_timeout`] (18seconds).
///
/// Ostensibly a little inside the 20s that a lot of routers might cut off at.
pub const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(18);

// We use a hard-coded server name for self-signed certificates.
pub(crate) const SERVER_NAME: &str = "maidsafe.net";

// Convenience alias – not for export.
type Result<T, E = ConfigError> = std::result::Result<T, E>;

/// Configuration errors.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// An error occurred when generating the TLS certificate.
    #[error("An error occurred when generating the TLS certificate")]
    // Though there are multiple different errors that could occur by the code, since we are
    // generating a certificate, they should only really occur due to buggy implementations. As
    // such, we don't attempt to expose more detail than 'something went wrong', which will
    // hopefully be enough for someone to file a bug report...
    CertificateGeneration(Box<dyn std::error::Error + Send + Sync>),
    /// Invalid idle timeout
    #[error("An error occurred parsing idle timeout duration")]
    InvalidIdleTimeout(#[from] quinn_proto::VarIntBoundsExceeded),
    /// rustls error
    #[error("An error occurred within rustls")]
    Rustls(#[from] rustls::Error),
    /// rustls error
    #[error("An error occurred generaeting client config certificates")]
    Webpki,
}

impl From<rcgen::RcgenError> for ConfigError {
    fn from(error: rcgen::RcgenError) -> Self {
        Self::CertificateGeneration(error.into())
    }
}

/// QuicP2p configurations
#[cfg_attr(feature = "structopt", derive(StructOpt))]
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Config {
    /// External port number assigned to the socket address of the program.
    /// If this is provided, QP2p considers that the local port provided has been mapped to the
    /// provided external port number.
    #[cfg_attr(feature = "structopt", structopt(long))]
    pub external_port: Option<u16>,

    /// External IP address of the computer on the WAN. This field is mandatory if the node is the genesis node.
    /// In case of non-genesis nodes, the external IP address will be resolved using the Echo service.
    #[cfg_attr(feature = "structopt", structopt(long))]
    pub external_ip: Option<IpAddr>,

    /// How long to wait to hear from a peer before timing out a connection.
    ///
    /// In the absence of any keep-alive messages, connections will be closed if they remain idle
    /// for at least this duration.
    ///
    /// If unspecified, this will default to [`DEFAULT_IDLE_TIMEOUT`].
    #[serde(default)]
    #[cfg_attr(feature = "structopt", structopt(long, parse(try_from_str = parse_millis), value_name = "MILLIS"))]
    pub idle_timeout: Option<Duration>,

    /// Interval at which to send keep-alives to maintain otherwise idle connections.
    ///
    /// Keep-alives prevent otherwise idle connections from timing out.
    ///
    /// If unspecified, this will default to `None`, disabling keep-alives.
    #[serde(default)]
    #[cfg_attr(feature = "structopt", structopt(long, parse(try_from_str = parse_millis), value_name = "MILLIS"))]
    pub keep_alive_interval: Option<Duration>,

    /// Maximum number of incoming bidirectional streams that may be open concurrently
    ///
    /// Must be nonzero for the peer to open any bidirectional streams.
    #[cfg_attr(feature = "structopt", structopt(long))]
    pub max_concurrent_bidi_streams: Option<u32>,
}

#[cfg(feature = "structopt")]
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
}

impl InternalConfig {
    pub(crate) fn try_from_config(config: Config) -> Result<Self> {
        let default_idle_timeout: IdleTimeout = IdleTimeout::try_from(DEFAULT_IDLE_TIMEOUT)?;

        // let's convert our duration to quinn's IdleTimeout
        let idle_timeout = config
            .idle_timeout
            .map(IdleTimeout::try_from)
            .unwrap_or(Ok(default_idle_timeout))
            .map_err(ConfigError::from)?;
        let keep_alive_interval = config.keep_alive_interval;

        let transport = Self::new_transport_config(
            idle_timeout,
            keep_alive_interval,
            config.max_concurrent_bidi_streams,
        );

        // setup certificates
        let mut roots = rustls::RootCertStore::empty();
        let (cert, key) = Self::generate_cert()?;
        roots.add(&cert).map_err(|_e| ConfigError::Webpki)?;

        let mut client_crypto = ClientConfig::builder()
            .with_safe_defaults()
            .with_root_certificates(roots)
            .with_no_client_auth();

        // allow client to connect to unknown certificates, eg those generated above
        client_crypto
            .dangerous()
            .set_certificate_verifier(Arc::new(SkipCertificateVerification));

        let mut server = quinn::ServerConfig::with_single_cert(vec![cert], key)?;
        server.transport = transport.clone();

        let mut client = quinn::ClientConfig::new(Arc::new(client_crypto));
        let _ = client.transport_config(transport);

        Ok(Self { client, server })
    }

    fn new_transport_config(
        idle_timeout: IdleTimeout,
        keep_alive_interval: Option<Duration>,
        max_concurrent_bidi_streams: Option<u32>,
    ) -> Arc<quinn::TransportConfig> {
        let mut config = quinn::TransportConfig::default();

        let _ = config.max_idle_timeout(Some(idle_timeout));

        let _ = config.keep_alive_interval(keep_alive_interval);
        if let Some(n) = max_concurrent_bidi_streams {
            let _ = config.max_concurrent_bidi_streams(VarInt::from_u32(n));
        }

        Arc::new(config)
    }

    fn generate_cert() -> Result<(Certificate, rustls::PrivateKey)> {
        let cert = rcgen::generate_simple_self_signed(vec![SERVER_NAME.to_string()])?;

        let key = cert.serialize_private_key_der();
        let cert = cert.serialize_der().unwrap();

        let key = rustls::PrivateKey(key);
        let cert = Certificate(cert);
        Ok((cert, key))
    }
}

struct SkipCertificateVerification;

impl rustls::client::ServerCertVerifier for SkipCertificateVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &Certificate,
        _intermediates: &[Certificate],
        _server_name: &ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp_response: &[u8],
        _now: std::time::SystemTime,
    ) -> Result<rustls::client::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::ServerCertVerified::assertion())
    }
}
