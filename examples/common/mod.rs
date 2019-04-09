use serde_derive::{Deserialize, Serialize};
use using_quinn::PeerInfo;

/// Remote procedure call for our examples to communicate.
#[derive(Debug, Serialize, Deserialize)]
pub enum Rpc {
    /// Starts the connectivity and data exchange test between us and given peers.
    StartTest(Vec<PeerInfo>),
}
