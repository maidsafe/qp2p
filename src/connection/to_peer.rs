// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::{
    connection::QConn,
    event::Event,
    peer::Peer,
    utils::{ConnectTerminator, Token},
    wire_msg::WireMsg,
    EventSenders,
};
use std::{fmt, net::SocketAddr};

/// Represent various stages of connection from us to the peer.
pub enum ToPeer {
    NoConnection,
    NotNeeded,
    Initiated {
        terminator: ConnectTerminator,
        peer_addr: SocketAddr,
        pending_sends: Vec<(WireMsg, Token)>,
        event_tx: EventSenders,
    },
    Established {
        q_conn: QConn,
    },
}

impl ToPeer {
    pub fn is_not_needed(&self) -> bool {
        if let ToPeer::NotNeeded = *self {
            true
        } else {
            false
        }
    }

    pub fn is_no_connection(&self) -> bool {
        if let ToPeer::NoConnection = *self {
            true
        } else {
            false
        }
    }

    #[allow(unused)]
    pub fn is_initiated(&self) -> bool {
        if let ToPeer::Initiated { .. } = *self {
            true
        } else {
            false
        }
    }

    pub fn is_established(&self) -> bool {
        if let ToPeer::Established { .. } = *self {
            true
        } else {
            false
        }
    }
}

impl Default for ToPeer {
    fn default() -> Self {
        ToPeer::NoConnection
    }
}

impl fmt::Debug for ToPeer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            ToPeer::Initiated {
                ref pending_sends, ..
            } => write!(
                f,
                "ToPeer::Initiated with {} pending sends",
                pending_sends.len()
            ),
            ToPeer::Established { .. } => write!(f, "ToPeer::Established"),
            ToPeer::NoConnection => write!(f, "ToPeer::NoConnection"),
            ToPeer::NotNeeded => write!(f, "ToPeer::NotNeeded"),
        }
    }
}

impl Drop for ToPeer {
    fn drop(&mut self) {
        match self {
            ToPeer::NotNeeded | ToPeer::NoConnection | ToPeer::Established { .. } => {}
            ToPeer::Initiated {
                terminator,
                peer_addr,
                pending_sends,
                event_tx,
                ..
            } => {
                let _ = terminator.try_send(());
                for (wire_msg, token) in pending_sends.drain(..) {
                    // No need to log these as this will fire even when the QuicP2p handle is
                    // dropped and at that point there might be no one listening so sender will
                    // error out
                    if let WireMsg::UserMsg(msg) = wire_msg {
                        let _ = event_tx.send(Event::UnsentUserMessage {
                            peer: Peer::Node(*peer_addr),
                            msg,
                            token,
                        });
                    }
                }
            }
        }
    }
}
