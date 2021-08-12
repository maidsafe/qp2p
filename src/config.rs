// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::{
    error::{Error, Result},
    utils,
};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::{fmt, net::IpAddr, str::FromStr};
use structopt::StructOpt;

/// QuicP2p configurations
#[derive(Clone, Debug, Default, Serialize, Deserialize, Eq, PartialEq, StructOpt)]
#[structopt(rename_all = "kebab-case")]
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
    /// If we hear nothing from the peer in the given interval we declare it offline to us. If none
    /// supplied we'll default to the documented constant.
    ///
    /// The interval is in milliseconds. A value of 0 disables this feature.
    #[structopt(long)]
    pub idle_timeout_msec: Option<u64>,
    /// Interval to send keep-alives if we are idling so that the peer does not disconnect from us
    /// declaring us offline. If none is supplied we'll default to the documented constant.
    ///
    /// The interval is in milliseconds. A value of 0 disables this feature.
    #[structopt(long)]
    pub keep_alive_interval_msec: Option<u32>,
    /// Duration of a UPnP port mapping.
    #[structopt(long)]
    pub upnp_lease_duration: Option<u32>,
    /// How long to retry establishing connections and sending messages.
    ///
    /// The duration is in milliseconds. Setting this to 0 will effectively disable retries.
    #[structopt(long, default_value = "30000")]
    pub retry_duration_msec: u64,
}

/// Config that has passed validation.
///
/// Generally this is a copy of [`Config`] without optional values where we would use defaults.
#[derive(Clone, Debug)]
pub(crate) struct InternalConfig {
    pub(crate) forward_port: bool,
    pub(crate) external_port: Option<u16>,
    pub(crate) external_ip: Option<IpAddr>,
    pub(crate) upnp_lease_duration: u32,
    pub(crate) retry_duration_msec: u64,
}

/// To be used to read and write our certificate and private key to disk esp. as a part of our
/// configuration file
#[derive(Serialize, Deserialize, Clone, Eq, PartialEq)]
pub(crate) struct SerialisableCertificate {
    /// DER encoded certificate
    cert_der: Bytes,
    /// DER encoded private key
    key_der: Bytes,
}

impl SerialisableCertificate {
    /// Returns a new Certificate that is valid for the list of domain names provided
    pub(crate) fn new(domains: impl Into<Vec<String>>) -> Result<Self> {
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
    pub(crate) fn obtain_priv_key_and_cert(
        &self,
    ) -> Result<(quinn::PrivateKey, quinn::Certificate)> {
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
