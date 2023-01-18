// Copyright 2021 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::{Config, Endpoint, IncomingConnections};
use bytes::Bytes;
use color_eyre::eyre::Result;
use rand::{distributions::Standard, rngs, Rng, SeedableRng};
use std::{
    net::{Ipv4Addr, SocketAddr},
    time::Duration,
};
use tiny_keccak::{Hasher, Sha3};

/// SHA3-256 hash digest.
type Digest256 = [u8; 32];

mod common;

#[ctor::ctor]
fn setup() {
    color_eyre::install().expect("color_eyre::install() should only be called once");
}

/// Construct an `Endpoint` with sane defaults for testing.
pub(crate) async fn new_endpoint_with_keepalive() -> Result<(Endpoint, IncomingConnections)> {
    Ok(Endpoint::new_peer(
        local_addr(),
        Config {
            keep_alive_interval: Some(Duration::from_secs(5)),
            ..Config::default()
        },
    )
    .await?)
}
/// Construct an `Endpoint` with sane defaults for testing.
pub(crate) async fn new_endpoint() -> Result<(Endpoint, IncomingConnections)> {
    Ok(Endpoint::new_peer(
        local_addr(),
        Config {
            ..Config::default()
        },
    )
    .await?)
}

pub(crate) fn local_addr() -> SocketAddr {
    (Ipv4Addr::LOCALHOST, 0).into()
}

pub(crate) fn random_msg(size: usize) -> Bytes {
    let random_bytes: Vec<u8> = rngs::SmallRng::from_entropy()
        .sample_iter(Standard)
        .take(size)
        .collect();
    Bytes::from(random_bytes)
}

pub(crate) fn hash(bytes: &Bytes) -> Digest256 {
    let mut hasher = Sha3::v256();
    let mut hash = Digest256::default();
    hasher.update(bytes);
    hasher.finalize(&mut hash);
    hash
}
