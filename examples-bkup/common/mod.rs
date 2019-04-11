// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use serde_derive::{Deserialize, Serialize};
use using_quinn::CrustInfo;

/// Remote procedure call for our examples to communicate.
#[derive(Debug, Serialize, Deserialize)]
pub enum Rpc {
    /// Starts the connectivity and data exchange test between us and given Crust peers.
    StartTest(Vec<CrustInfo>),
}
