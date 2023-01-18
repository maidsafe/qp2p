use std::{net::SocketAddr, sync::Arc, time::Duration};

use quinn::{IdleTimeout, TransportConfig, VarInt};
use tokio::sync::mpsc;

use crate::{endpoint::listen_for_incoming_connections, Endpoint, IncomingConnections};

/// Standard size of our channel bounds
const STANDARD_CHANNEL_SIZE: usize = 10_000;

/// Build a [`crate::Endpoint`]
#[allow(missing_debug_implementations)]
pub struct EndpointBuilder {
    addr: SocketAddr,
    max_idle_timeout: Option<IdleTimeout>,
    max_concurrent_bidi_streams: VarInt,
    max_concurrent_uni_streams: VarInt,
    keep_alive_interval: Option<Duration>,
}

impl Default for EndpointBuilder {
    fn default() -> Self {
        Self {
            addr: SocketAddr::from(([0, 0, 0, 0], 0)),
            max_idle_timeout: Some(IdleTimeout::from(VarInt::from_u32(10_000))), // 10s
            max_concurrent_bidi_streams: 100u32.into(),
            max_concurrent_uni_streams: 100u32.into(),
            keep_alive_interval: None,
        }
    }
}

impl EndpointBuilder {
    /// Instantiate a builder with default parameters.
    /// See source of [`Self::default`] for default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Local address passed to [`quinn::Endpoint::client`].
    pub fn addr(&mut self, addr: SocketAddr) -> &mut Self {
        self.addr = addr;
        self
    }

    /// Takes time in milliseconds.
    ///
    /// Maps to [`quinn::TransportConfig::max_idle_timeout`].
    pub fn max_idle_timeout(&mut self, to: Option<u32>) -> &mut Self {
        self.max_idle_timeout = to.map(|v| IdleTimeout::from(VarInt::from_u32(v)));
        self
    }

    /// Takes time in milliseconds.
    ///
    /// Maps to [`quinn::TransportConfig::max_concurrent_bidi_streams`].
    pub fn max_concurrent_bidi_streams(&mut self, max: u32) -> &mut Self {
        self.max_concurrent_bidi_streams = VarInt::from_u32(max);
        self
    }

    /// Takes time in milliseconds.
    ///
    /// Maps to [`quinn::TransportConfig::max_concurrent_uni_streams`].
    pub fn max_concurrent_uni_streams(&mut self, max: u32) -> &mut Self {
        self.max_concurrent_uni_streams = VarInt::from_u32(max);
        self
    }

    /// Maps to [`quinn::TransportConfig::keep_alive_interval`].
    pub fn keep_alive_interval(&mut self, interval: Option<Duration>) -> &mut Self {
        self.keep_alive_interval = interval;
        self
    }

    /// Instantiate a server (peer) [`Endpoint`] using the parameters passed to this builder.
    pub async fn server(self) -> Result<(Endpoint, IncomingConnections), EndpointError> {
        let (cfg_srv, cfg_cli) = self.config()?;

        let mut endpoint = quinn::Endpoint::server(cfg_srv, self.addr)?;
        endpoint.set_default_client_config(cfg_cli);

        let (connection_tx, connection_rx) = mpsc::channel(STANDARD_CHANNEL_SIZE);
        listen_for_incoming_connections(endpoint.clone(), connection_tx);

        Ok((
            Endpoint {
                local_addr: self.addr,
                inner: endpoint,
            },
            IncomingConnections(connection_rx),
        ))
    }

    /// Instantiate a client (unreachable) [`Endpoint`] using the parameters passed to this builder.
    pub async fn client(self) -> Result<Endpoint, EndpointError> {
        let (_, cfg_cli) = self.config()?;

        let mut endpoint = quinn::Endpoint::client(self.addr)?;
        endpoint.set_default_client_config(cfg_cli);

        Ok(Endpoint {
            local_addr: self.addr,
            inner: endpoint,
        })
    }

    /// Helper to construct a [`TransportConfig`] from our parameters.
    fn transport_config(&self) -> TransportConfig {
        let mut config = TransportConfig::default();
        let _ = config.max_idle_timeout(self.max_idle_timeout);
        let _ = config.keep_alive_interval(self.keep_alive_interval);
        let _ = config.max_concurrent_bidi_streams(self.max_concurrent_bidi_streams);
        let _ = config.max_concurrent_uni_streams(self.max_concurrent_uni_streams);

        config
    }

    fn config(&self) -> Result<(quinn::ServerConfig, quinn::ClientConfig), EndpointError> {
        let transport = Arc::new(self.transport_config());

        let (mut server, mut client) = config().map_err(EndpointError::Certificate)?;
        let _ = server.transport_config(Arc::clone(&transport));
        let _ = client.transport_config(Arc::clone(&transport));

        Ok((server, client))
    }
}

#[derive(thiserror::Error, Debug)]
pub enum EndpointError {
    #[error("Certificate could not be generated for config")]
    Certificate(CertificateError),

    #[error("Failed to bind UDP socket")]
    Socket(#[from] std::io::Error),
}

#[derive(thiserror::Error, Debug)]
pub enum CertificateError {
    #[error("Rcgen internal error generating certificate")]
    Rcgen(#[from] rcgen::RcgenError),
    #[error("Certificate or name validation error")]
    WebPki(#[from] webpki::Error),
    #[error("Rustls internal error")]
    Rustls(#[from] rustls::Error),
}

// We use a hard-coded server name for self-signed certificates.
pub(crate) const SERVER_NAME: &str = "maidsafe.net";

fn config() -> Result<(quinn::ServerConfig, quinn::ClientConfig), CertificateError> {
    let mut roots = rustls::RootCertStore::empty();
    let (cert, key) = generate_cert()?;
    roots.add(&cert)?;

    let mut client_crypto = rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_root_certificates(roots)
        .with_no_client_auth();

    // allow client to connect to unknown certificates, eg those generated above
    client_crypto
        .dangerous()
        .set_certificate_verifier(Arc::new(SkipServerVerification));

    let server = quinn::ServerConfig::with_single_cert(vec![cert], key)?;
    let client = quinn::ClientConfig::new(Arc::new(client_crypto));

    Ok((server, client))
}

fn generate_cert() -> Result<(rustls::Certificate, rustls::PrivateKey), rcgen::RcgenError> {
    let cert = rcgen::generate_simple_self_signed(vec![SERVER_NAME.to_string()])?;

    let key = cert.serialize_private_key_der();
    let cert = cert.serialize_der().unwrap();

    let key = rustls::PrivateKey(key);
    let cert = rustls::Certificate(cert);
    Ok((cert, key))
}

// Implementation of `ServerCertVerifier` that verifies everything as trustworthy.
struct SkipServerVerification;

impl rustls::client::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::Certificate,
        _intermediates: &[rustls::Certificate],
        _server_name: &rustls::ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp_response: &[u8],
        _now: std::time::SystemTime,
    ) -> Result<rustls::client::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::ServerCertVerified::assertion())
    }
}
