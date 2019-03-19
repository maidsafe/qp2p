use crate::CrustInfo;

/// Crust configurations
#[derive(Default, Serialize, Deserialize)]
pub struct Config {
    /// Hard Coded contacts
    pub hard_coded_contacts: Vec<CrustInfo>,
    /// Port we want to reserve for QUIC. If none supplied we'll use the OS given random port.
    pub port: Option<u16>,
    /// This is the maximum message size we'll allow the peer to send to us. Any bigger message and
    /// we'll error out probably shutting down the connection to the peer. If none supplied we'll
    /// default to the documented constant.
    pub max_msg_size_allowed: Option<u32>,
    /// Path to our TLS Certificate. This file must contain `SerialisableCeritificate` as content
    pub our_complete_cert: Option<SerialisableCeritificate>,
}

impl Config {
    pub fn read_or_construct_default() -> Config {
        // FIXME 1st try reading from the disk
        Default::default()
    }

    pub fn with_default_cert() -> Config {
        Self {
            hard_coded_contacts: Default::default(),
            port: Default::default(),
            max_msg_size_allowed: Default::default(),
            our_complete_cert: Some(Default::default()),
        }
    }
}

/// To be used to read and write our certificate and private key to disk esp. as a part of our
/// configuration file
#[derive(Serialize, Deserialize, Clone)]
pub struct SerialisableCeritificate {
    pub cert_der: Vec<u8>,
    pub key_der: Vec<u8>,
}

impl SerialisableCeritificate {
    // TODO do proper error handling
    pub fn obtain_priv_key_and_cert(&self) -> (quinn::PrivateKey, quinn::Certificate) {
        (
            unwrap!(quinn::PrivateKey::from_der(&self.key_der)),
            unwrap!(quinn::Certificate::from_der(&self.cert_der)),
        )
    }
}

impl Default for SerialisableCeritificate {
    fn default() -> Self {
        let cert = rcgen::generate_simple_self_signed(vec!["MaidSAFE.net".to_string()]);

        Self {
            cert_der: cert.serialize_der(),
            key_der: cert.serialize_private_key_der(),
        }
    }
}
