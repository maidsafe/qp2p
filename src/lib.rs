// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

//! qp2p enables communication within a peer to peer network over the QUIC protocol.

// For explanation of lint checks, run `rustc -W help`
// Forbid some very bad patterns. Forbid is stronger than `deny`, preventing us from suppressing the
// lint with `#[allow(...)]` et-all.
#![forbid(
    arithmetic_overflow,
    mutable_transmutes,
    no_mangle_const_items,
    unknown_crate_types,
    unsafe_code
)]
// Turn on some additional warnings to encourage good style.
#![warn(
    missing_debug_implementations,
    missing_docs,
    trivial_casts,
    trivial_numeric_casts,
    unreachable_pub,
    unused_extern_crates,
    unused_import_braces,
    unused_qualifications,
    unused_results,
    clippy::unicode_not_nfc
)]

pub mod config;
mod connection;
mod endpoint;
mod error;
mod utils;
mod wire_msg;

pub use config::{Config, ConfigError};
pub use connection::{Connection, RecvStream, SendStream};
pub use endpoint::{Endpoint, IncomingConnections};
pub use error::{
    ClientEndpointError, Close, ConnectionError, EndpointError, InternalConfigError, RecvError,
    RpcError, SendError, StreamError, TransportErrorCode, UnsupportedStreamOperation,
};
pub use wire_msg::UsrMsgBytes;

#[cfg(test)]
mod tests;
