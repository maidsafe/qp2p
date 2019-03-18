use crate::CrustInfo;

/// Crust configurations
#[derive(Default)]
pub struct Config {
    /// Hard Coded contacts
    pub hard_coded_contacts: Vec<CrustInfo>,
    /// Port we want to reserve for QUIC
    pub port: Option<u16>,
    /// Our TLS Certificate
    pub cert: Certificate,
}

/// Certificate
#[derive(Clone)]
pub struct Certificate {
    pub key: quinn::PrivateKey,
    pub cert: quinn::Certificate,
    pub cert_der: Vec<u8>, // TODO only because quinn does not expose the inner Vec<u8> unlike tls
}

impl Default for Certificate {
    fn default() -> Self {
        let cert = rcgen::generate_simple_self_signed(vec!["MaidSAFE.net".to_string()]);
        let cert_der = cert.serialize_der();

        Self {
            key: unwrap!(quinn::PrivateKey::from_der(
                &cert.serialize_private_key_der()
            )),
            cert: unwrap!(quinn::Certificate::from_der(&cert_der)),
            cert_der,
        }
    }
}
