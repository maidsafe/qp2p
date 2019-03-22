use serde_derive::{Deserialize, Serialize};
use using_quinn::CrustInfo;

/// Remote procedure call for our examples to communicate.
#[derive(Serialize, Deserialize)]
pub enum Rpc {
    /// Starts the connectivity and data exchange test between us and given Crust peers.
    StartTest(Vec<CrustInfo>),
}
