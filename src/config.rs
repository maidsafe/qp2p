// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::NodeInfo;
use std::net::IpAddr;

/// Crust configurations
#[derive(Default, Serialize, Deserialize)]
pub struct Config {
    /// Hard Coded contacts
    pub hard_coded_contacts: Vec<NodeInfo>,
    /// Port we want to reserve for QUIC. If none supplied we'll use the OS given random port.
    pub port: Option<u16>,
    /// IP address for the listener. If none supplied we'll use the default address (0.0.0.0).
    pub ip: Option<IpAddr>,
    /// This is the maximum message size we'll allow the peer to send to us. Any bigger message and
    /// we'll error out probably shutting down the connection to the peer. If none supplied we'll
    /// default to the documented constant.
    pub max_msg_size_allowed: Option<u32>,
    /// If we hear nothing from the peer in the given interval we declare it offline to us. If none
    /// supplied we'll default to the documented constant.
    ///
    /// The interval is in milliseconds. A value of 0 disables this feature.
    pub idle_timeout_msec: Option<u64>,
    /// Interval to send keep-alives if we are idling so that the peer does not disconnect from us
    /// declaring us offline. If none is supplied we'll default to the documented constant.
    ///
    /// The interval is in milliseconds. A value of 0 disables this feature.
    pub keep_alive_interval_msec: Option<u32>,
    /// Path to our TLS Certificate. This file must contain `SerialisableCertificate` as content
    pub our_complete_cert: Option<SerialisableCertificate>,
    /// Specify if we are a client or a node
    pub our_type: OurType,
}

impl Config {
    pub fn read_or_construct_default() -> Config {
        // FIXME 1st try reading from the disk
        Default::default()
    }

    pub fn with_default_cert() -> Config {
        Self {
            our_complete_cert: Some(Default::default()),
            ..Self::default()
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

/// Whether we are a client or a node
#[derive(Debug, Serialize, Deserialize, Clone, Copy, Eq, PartialEq)]
pub enum OurType {
    /// We are a client
    Client,
    /// We are a node
    Node,
}

impl Default for OurType {
    fn default() -> Self {
        OurType::Node
    }
}
