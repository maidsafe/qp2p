// Copyright 2021 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::{
    Config, ConnId, Connection, Endpoint, IncomingConnections, IncomingMessages, RetryConfig,
};
use bytes::Bytes;
use color_eyre::eyre::Result;
use std::{
    net::{Ipv4Addr, SocketAddr},
    time::Duration,
};
use tiny_keccak::{Hasher, Sha3};

/// SHA3-256 hash digest.
type Digest256 = [u8; 32];

mod common;
mod quinn;

#[ctor::ctor]
fn setup() {
    color_eyre::install().expect("color_eyre::install() should only be called once");
}

impl ConnId for [u8; 32] {
    fn generate(_socket_addr: &SocketAddr) -> Self {
        rand::random()
    }
}

/// Construct an `Endpoint` with sane defaults for testing.
pub(crate) async fn new_endpoint() -> Result<(
    Endpoint<[u8; 32]>,
    IncomingConnections<[u8; 32]>,
    IncomingMessages<[u8; 32]>,
    Option<Connection<[u8; 32]>>,
)> {
    Ok(Endpoint::new(
        local_addr(),
        &[],
        Config {
            retry_config: RetryConfig {
                retrying_max_elapsed_time: Duration::from_millis(500),
                ..RetryConfig::default()
            },
            ..Config::default()
        },
    )
    .await?)
}

pub(crate) fn local_addr() -> SocketAddr {
    (Ipv4Addr::LOCALHOST, 0).into()
}

pub(crate) fn random_msg(size: usize) -> Bytes {
    let random_bytes: Vec<u8> = (0..size).map(|_| rand::random::<u8>()).collect();
    Bytes::from(random_bytes)
}

pub(crate) fn hash(bytes: &Bytes) -> Digest256 {
    let mut hasher = Sha3::v256();
    let mut hash = Digest256::default();
    hasher.update(bytes);
    hasher.finalize(&mut hash);
    hash
}
