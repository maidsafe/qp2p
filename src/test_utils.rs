// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use rand::Rng;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::PathBuf;

pub(crate) fn test_dirs() -> PathBuf {
    tmp_rand_dir()
}

pub(crate) fn rand_node_addr() -> SocketAddr {
    let mut rng = rand::thread_rng();
    make_node_addr(rng.gen())
}

pub(crate) fn make_node_addr(port: u16) -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
}

fn tmp_rand_dir() -> PathBuf {
    let fname = format!("qp2p_tests_{:016x}", rand::random::<u64>());
    let mut path = std::env::temp_dir();
    path.push(fname);
    path
}
