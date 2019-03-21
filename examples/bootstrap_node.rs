//! Bootstrap node which acts as a relay for other client nodes. It collects the info of multiple
//! client nodes and relays it to all remaining connected nodes, hence allows them all to connect
//! with each other.

mod common;
use common::Rpc;

use using_quinn::{Config, Crust, CrustInfo, Event};

fn main() {

}
