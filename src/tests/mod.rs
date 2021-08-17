// Copyright 2021 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::{Config, ConnId, QuicP2p};
use anyhow::Result;
use bytes::Bytes;
use std::{
    net::{Ipv4Addr, SocketAddr},
    time::Duration,
};
use tiny_keccak::{Hasher, Sha3};

/// SHA3-256 hash digest.
type Digest256 = [u8; 32];

mod common;
mod quinn;

impl ConnId for [u8; 32] {
    fn generate(_socket_addr: &SocketAddr) -> Self {
        rand::random()
    }
}

/// Constructs a `QuicP2p` node with some sane defaults for testing.
pub(crate) fn new_qp2p() -> Result<QuicP2p<[u8; 32]>> {
    let qp2p = QuicP2p::<[u8; 32]>::with_config(Config {
        // turn down the retry duration - we won't live forever
        // note that this would just limit retries, UDP connection attempts seem to take 60s to
        // timeout
        min_retry_duration: Duration::from_millis(500).into(),
        ..Config::default()
    })?;

    Ok(qp2p)
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
