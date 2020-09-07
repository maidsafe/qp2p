// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

//! quic-p2p enables communication within a peer to peer network over the QUIC protocol.

// For explanation of lint checks, run `rustc -W help`
#![forbid(
    arithmetic_overflow,
    mutable_transmutes,
    no_mangle_const_items,
    unknown_crate_types,
    //warnings
)]
#![deny(
    bad_style,
    deprecated,
    improper_ctypes,
    missing_docs,
    non_shorthand_field_patterns,
    overflowing_literals,
    stable_features,
    unconditional_recursion,
    unknown_lints,
    unsafe_code,
    unused_allocation,
    unused_attributes,
    unused_comparisons,
    unused_features,
    unused_parens,
    while_true,
    clippy::unicode_not_nfc,
    clippy::wrong_pub_self_convention,
    clippy::unwrap_used
)]
#![warn(
    trivial_casts,
    trivial_numeric_casts,
    unused,
    unused_extern_crates,
    unused_import_braces,
    unused_qualifications,
    unused_results
)]

mod api;
mod bootstrap_cache;
mod config;
mod connections;
mod dirs;
mod endpoint;
mod error;
#[cfg(feature = "upnp")]
mod igd;
mod peer_config;
#[cfg(test)]
mod test_utils;
mod utils;
mod wire_msg;

#[cfg(feature = "upnp")]
pub use crate::igd::{DEFAULT_UPNP_LEASE_DURATION_SEC, UPNP_RESPONSE_TIMEOUT_MSEC};
pub use api::{Message, QuicP2p};
pub use config::Config;
pub use connections::{Connection, IncomingConnections, IncomingMessages, RecvStream, SendStream};
pub use dirs::{Dirs, OverRide};
pub use endpoint::Endpoint;
pub use error::{Error, Result};
pub use peer_config::{DEFAULT_IDLE_TIMEOUT_MSEC, DEFAULT_KEEP_ALIVE_INTERVAL_MSEC};
