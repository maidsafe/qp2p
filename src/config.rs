use crate::CrustInfo;
use std::net::Ipv4Addr;

/// Crust configurations
#[derive(Default, Serialize, Deserialize)]
pub struct Config {
    /// Hard Coded contacts
    pub hard_coded_contacts: Vec<CrustInfo>,
    /// Port we want to reserve for QUIC. If none supplied we'll use the OS given random port.
    pub port: Option<u16>,
    /// IP address for the listener. If none supplied we'll use the default address (0.0.0.0).
    pub ip: Option<Ipv4Addr>,
    /// This is the maximum message size we'll allow the peer to send to us. Any bigger message and
    /// we'll error out probably shutting down the connection to the peer. If none supplied we'll
    /// default to the documented constant.
    pub max_msg_size_allowed: Option<u32>,
    /// If we hear nothing from the peer in the given interval we declare it offline to us. If none
    /// supplied we'll default to the documented constant.
    ///
    /// The interval is in seconds. A value of 0 disables this feature.
    pub idle_timeout: Option<u64>,
    /// Interval to send keep-alives if we are idling so that the peer does not disconnect from us
    /// declaring us offline. If none is supplied we'll default to the documented constant.
    ///
    /// The interval is in seconds. A value of 0 disables this feature.
    pub keep_alive_interval: Option<u32>,
    /// Path to our TLS Certificate. This file must contain `SerialisableCertificate` as content
    pub our_complete_cert: Option<SerialisableCertificate>,
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
            ip: Default::default(),
            max_msg_size_allowed: Default::default(),
            idle_timeout: Default::default(),
            keep_alive_interval: Default::default(),
            our_complete_cert: Some(Default::default()),
        }
    }
}

/// To be used to read and write our certificate and private key to disk esp. as a part of our
/// configuration file
#[derive(Serialize, Deserialize, Clone)]
pub struct SerialisableCertificate {
    pub cert_der: Vec<u8>,
    pub key_der: Vec<u8>,
}

impl SerialisableCertificate {
    // TODO do proper error handling
    pub fn obtain_priv_key_and_cert(&self) -> (quinn::PrivateKey, quinn::Certificate) {
        (
            unwrap!(quinn::PrivateKey::from_der(&self.key_der)),
            unwrap!(quinn::Certificate::from_der(&self.cert_der)),
        )
    }
}

impl Default for SerialisableCertificate {
    fn default() -> Self {
        let cert = rcgen::generate_simple_self_signed(vec!["MaidSAFE.net".to_string()]);

        Self {
            cert_der: cert.serialize_der(),
            key_der: cert.serialize_private_key_der(),
        }
    }
}
