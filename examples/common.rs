use using_quinn::CrustInfo;

/// Remote procedure call for our examples to communicate.
pub enum Rpc {
    /// Starts the connectivity and data exchange test between us and given Crust peers.
    StartTest(Vec<CrustInfo>)
}
