// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::dirs::Dirs;
use crate::error::QuicP2pError;
use crate::utils;
use crate::R;
use base64;
use bincode;
use bytes::Bytes;
use log::{trace, warn};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet, fmt, fs, io, net::IpAddr, net::SocketAddr, path::PathBuf, str::FromStr,
};
use structopt::StructOpt;
use unwrap::unwrap;

/// QuicP2p configurations
#[derive(Clone, Debug, Default, Serialize, Deserialize, Eq, PartialEq, StructOpt)]
#[structopt(rename_all = "kebab-case")]
pub struct Config {
    /// Hard Coded contacts
    #[structopt(
        short,
        long,
        default_value = "[]",
        parse(try_from_str = "serde_json::from_str")
    )]
    pub hard_coded_contacts: HashSet<SocketAddr>,
    /// Port we want to reserve for QUIC. If none supplied we'll use the OS given random port.
    #[structopt(short, long)]
    pub port: Option<u16>,
    /// IP address for the listener. If none supplied we'll use the default address (0.0.0.0).
    #[structopt(long)]
    pub ip: Option<IpAddr>,
    /// This is the maximum message size we'll allow the peer to send to us. Any bigger message and
    /// we'll error out probably shutting down the connection to the peer. If none supplied we'll
    /// default to the documented constant.
    #[structopt(long)]
    pub max_msg_size_allowed: Option<u32>,
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
    /// Our TLS Certificate. If passed as a command line argument, it should be encoded in base64
    /// (see `SerialisableCertificate::to_string`).
    #[structopt(long, parse(try_from_str))]
    pub our_complete_cert: Option<SerialisableCertificate>,
    /// Directory in which the bootstrap cache will be stored. If none is supplied, the platform specific
    /// default cache directory is used.
    #[structopt(long)]
    pub bootstrap_cache_dir: Option<String>,
    /// Specify if we are a client or a node
    #[structopt(short = "t", long, default_value = "node")]
    pub our_type: OurType,
}

impl Config {
    /// Try and read the config off the disk first. If such a file-path doesn't exist it'll create
    /// a default one with random certificate and write that to the disk, eventually returning that
    /// config to the caller.
    pub fn read_or_construct_default(user_override: Option<&Dirs>) -> R<Config> {
        let config_path = config_path(user_override)?;

        if config_path.exists() {
            trace!("Reading config from {:?}", config_path);
            Ok(utils::read_from_disk(&config_path)?)
        } else {
            let config_dir = config_path
                .parent()
                .ok_or_else(|| io::ErrorKind::NotFound.into())
                .map_err(QuicP2pError::Io)?;
            fs::create_dir_all(&config_dir)?;

            let cfg = Config::with_default_cert();
            utils::write_to_disk(&config_path, &cfg)?;

            Ok(cfg)
        }
    }

    /// Create a default Config with random Certificate
    pub fn with_default_cert() -> Config {
        trace!("Constructing default Config");

        Self {
            our_complete_cert: Some(Default::default()),
            ..Self::default()
        }
    }
}

/// To be used to read and write our certificate and private key to disk esp. as a part of our
/// configuration file
#[derive(Serialize, Deserialize, Clone, Eq, PartialEq)]
pub struct SerialisableCertificate {
    /// DER encoded certificate
    pub cert_der: Bytes,
    /// DER encoded private key
    pub key_der: Bytes,
}

impl SerialisableCertificate {
    /// Parses DER encoded binary key material to a format that can be used by Quinn
    ///
    /// # Errors
    /// Returns [CertificateParseError](enum.Error.html#variant.CertificateParseError) if the inputs
    /// cannot be parsed
    pub fn obtain_priv_key_and_cert(&self) -> R<(quinn::PrivateKey, quinn::Certificate)> {
        Ok((
            quinn::PrivateKey::from_der(&self.key_der)
                .map_err(|_| QuicP2pError::CertificateParseError)?,
            quinn::Certificate::from_der(&self.cert_der)
                .map_err(|_| QuicP2pError::CertificateParseError)?,
        ))
    }
}

impl Default for SerialisableCertificate {
    fn default() -> Self {
        let cert = unwrap!(rcgen::generate_simple_self_signed(vec![
            "MaidSAFE.net".to_string()
        ]));

        Self {
            cert_der: unwrap!(cert.serialize_der()).into(),
            key_der: cert.serialize_private_key_der().into(),
        }
    }
}

impl FromStr for SerialisableCertificate {
    type Err = QuicP2pError;

    /// Decode `SerialisableCertificate` from a base64-encoded string.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let cert = base64::decode(s)?;
        let (cert_der, key_der) = bincode::deserialize(&cert)?;
        Ok(Self { cert_der, key_der })
    }
}

impl ToString for SerialisableCertificate {
    /// Convert `SerialisableCertificate` into a base64-encoded string.
    fn to_string(&self) -> String {
        let cert = unwrap!(bincode::serialize(&(&self.cert_der, &self.key_der)));
        base64::encode(&cert)
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

/// Whether we are a client or a node
#[derive(Debug, Serialize, Deserialize, Clone, Copy, Eq, PartialEq)]
pub enum OurType {
    /// We are a client
    Client,
    /// We are a node
    Node,
}

impl FromStr for OurType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "client" => Ok(OurType::Client),
            "node" => Ok(OurType::Node),
            x => {
                let err = format!("Unknown client type: {}", x);
                warn!("{}", err);
                Err(err)
            }
        }
    }
}

impl Default for OurType {
    fn default() -> Self {
        OurType::Node
    }
}

fn config_path(user_override: Option<&Dirs>) -> R<PathBuf> {
    let path = |dir: &Dirs| {
        let path = dir.config_dir();
        path.join("config")
    };

    let cfg_path = user_override.map_or_else(
        || Ok::<_, QuicP2pError>(path(&utils::project_dir()?)),
        |d| Ok(path(d)),
    )?;

    Ok(cfg_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::test_dirs;
    use crate::utils;

    #[test]
    fn config_create_read_and_write() {
        let dir = test_dirs();
        let config_path = unwrap!(config_path(Some(&dir)));

        assert!(utils::read_from_disk::<Config>(&config_path).is_err());

        let cfg = unwrap!(Config::read_or_construct_default(Some(&dir)));
        let read_cfg = unwrap!(utils::read_from_disk(&config_path));

        assert_eq!(cfg, read_cfg);
    }
}
