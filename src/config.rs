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
use log::trace;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::{
    collections::HashSet, fmt, fs, io, net::IpAddr, net::SocketAddr, path::PathBuf, str::FromStr,
};
use structopt::StructOpt;

/// QuicP2p configurations
#[derive(Clone, Debug, Default, Serialize, Deserialize, Eq, PartialEq, StructOpt)]
#[structopt(rename_all = "kebab-case")]
pub struct Config {
    /// Hard Coded contacts
    #[structopt(
        short,
        long,
        default_value = "[]",
        parse(try_from_str = serde_json::from_str)
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
    /// Directory in which the bootstrap cache will be stored. If none is supplied, the platform specific
    /// default cache directory is used.
    #[structopt(long)]
    pub bootstrap_cache_dir: Option<String>,
    /// Duration of a UPnP port mapping.
    #[structopt(long)]
    pub upnp_lease_duration: Option<u32>,
    /// Specify if port forwarding via UPnP should be done or not
    #[structopt(long)]
    pub forward_port: bool,
    /// Use a fresh config without re-using any config available on disk
    #[structopt(long)]
    pub fresh: bool,
    /// Clean all existing config available on disk
    #[structopt(long)]
    pub clean: bool,
}

impl Config {
    /// Try and read the config off the disk first. If such a file-path doesn't exist it'll create
    /// a default one with random certificate and write that to the disk, eventually returning that
    /// config to the caller.
    pub fn read_or_construct_default(user_override: Option<&Path>) -> Result<Config> {
        let config_path = config_path(user_override)?;

        if config_path.exists() {
            trace!("Reading config from {:?}", config_path);
            Ok(utils::read_from_disk(&config_path)?)
        } else {
            let config_dir = config_path
                .parent()
                .ok_or_else(|| io::ErrorKind::NotFound.into())
                .map_err(Error::Io)?;
            fs::create_dir_all(&config_dir)?;

            let cfg = Config::default();
            utils::write_to_disk(&config_path, &cfg)?;

            Ok(cfg)
        }
    }

    /// Clear all configuration files from disk
    pub fn clear_config_from_disk(user_override: Option<&Path>) -> Result<()> {
        let config_path = config_path(user_override)?;
        if config_path.exists() {
            fs::remove_file(&config_path)?;
        }
        Ok(())
    }
}

/// To be used to read and write our certificate and private key to disk esp. as a part of our
/// configuration file
#[derive(Serialize, Deserialize, Clone, Eq, PartialEq)]
pub(crate) struct SerialisableCertificate {
    /// DER encoded certificate
    pub cert_der: Bytes,
    /// DER encoded private key
    pub key_der: Bytes,
}

impl SerialisableCertificate {
    /// Returns a new Certificate that is valid for the list of domain names provided
    pub fn new(domains: impl Into<Vec<String>>) -> Result<Self> {
        let cert = rcgen::generate_simple_self_signed(domains)
            .map_err(|_| Error::Unexpected("Error generating certificate".to_string()))?;

        Ok(Self {
            cert_der: cert
                .serialize_der()
                .map_err(|_| {
                    Error::Unexpected(
                        "Error serializing certificate into binary DER format".to_string(),
                    )
                })?
                .into(),
            key_der: cert.serialize_private_key_der().into(),
        })
    }

    /// Parses DER encoded binary key material to a format that can be used by Quinn
    ///
    /// # Errors
    /// Returns [CertificateParseError](enum.Error.html#variant.CertificateParseError) if the inputs
    /// cannot be parsed
    pub fn obtain_priv_key_and_cert(&self) -> Result<(quinn::PrivateKey, quinn::Certificate)> {
        Ok((
            quinn::PrivateKey::from_der(&self.key_der).map_err(|_| Error::CertificateParseError)?,
            quinn::Certificate::from_der(&self.cert_der)
                .map_err(|_| Error::CertificateParseError)?,
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

fn config_path(user_override: Option<&Path>) -> Result<PathBuf> {
    let get_config_file = |dir: &Path| dir.join("config");

    let cfg_path = user_override.map_or_else(
        || Ok::<_, Error>(get_config_file(&utils::project_dir()?)),
        |d| Ok(get_config_file(d)),
    )?;

    Ok(cfg_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::test_dirs;
    use crate::{utils, Error};

    #[test]
    fn config_create_read_and_write() -> Result<(), Error> {
        let dir = test_dirs();
        let config_path = config_path(Some(&dir))?;

        assert!(utils::read_from_disk::<Config>(&config_path).is_err());

        let cfg = Config::read_or_construct_default(Some(&dir))?;
        let read_cfg = utils::read_from_disk(&config_path)?;

        assert_eq!(cfg, read_cfg);
        Ok(())
    }
}
