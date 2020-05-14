// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::{ctx_mut, dirs::Dirs, error::QuicP2pError, event::Event, peer::Peer, EventSenders};
use crossbeam_channel as mpmc;
use log::debug;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

/// Result used by `QuicP2p`.
pub type R<T> = Result<T, QuicP2pError>;
/// Token accepted during sends and returned back to the user to help identify the context.
pub type Token = u64;

/// This is to terminate the connection attempt should it take too long to mature to completeness.
pub type ConnectTerminator = tokio::sync::mpsc::Sender<()>;
/// Obtain a `ConnectTerminator` paired with a corresponding receiver.
pub fn connect_terminator() -> (ConnectTerminator, tokio::sync::mpsc::Receiver<()>) {
    tokio::sync::mpsc::channel(1)
}

pub(crate) struct EventReceivers {
    pub node_rx: mpmc::Receiver<Event>,
    pub client_rx: mpmc::Receiver<Event>,
}

impl EventReceivers {
    #![allow(unused)]
    pub fn recv(&self) -> Result<Event, mpmc::RecvError> {
        let mut sel = mpmc::Select::new();
        let client_idx = sel.recv(&self.client_rx);
        let node_idx = sel.recv(&self.node_rx);
        let selected_operation = sel.ready();

        if selected_operation == client_idx {
            self.client_rx.recv()
        } else if selected_operation == node_idx {
            self.node_rx.recv()
        } else {
            panic!("invalid operation");
        }
    }

    pub fn recv_timeout(&self, timeout: Duration) -> Result<Event, QuicP2pError> {
        let mut sel = mpmc::Select::new();
        let client_idx = sel.recv(&self.client_rx);
        let node_idx = sel.recv(&self.node_rx);
        let selected_operation = sel.ready_timeout(timeout).map_err(|_| mpmc::RecvError)?;

        if selected_operation == client_idx {
            Ok(self.client_rx.recv()?)
        } else if selected_operation == node_idx {
            Ok(self.node_rx.recv()?)
        } else {
            panic!("invalid operation");
        }
    }

    #[allow(unused)]
    pub fn try_recv(&self) -> Result<Event, mpmc::TryRecvError> {
        self.node_rx
            .try_recv()
            .or_else(|_| self.client_rx.try_recv())
    }

    #[allow(unused)]
    pub fn iter(&self) -> IterEvent {
        IterEvent { event_rx: &self }
    }
}

pub struct IterEvent<'a> {
    event_rx: &'a EventReceivers,
}

impl<'a> Iterator for IterEvent<'a> {
    type Item = Event;

    fn next(&mut self) -> Option<Self::Item> {
        let mut sel = mpmc::Select::new();
        let client_idx = sel.recv(&self.event_rx.client_rx);
        let node_idx = sel.recv(&self.event_rx.node_rx);
        let selected_operation = sel.ready();

        let event = if selected_operation == client_idx {
            self.event_rx.client_rx.recv()
        } else if selected_operation == node_idx {
            self.event_rx.node_rx.recv()
        } else {
            return None;
        };

        event.ok()
    }
}

pub(crate) fn new_unbounded_channels() -> (EventSenders, EventReceivers) {
    let (client_tx, client_rx) = mpmc::unbounded();
    let (node_tx, node_rx) = mpmc::unbounded();
    (
        EventSenders { node_tx, client_tx },
        EventReceivers { node_rx, client_rx },
    )
}

/// Get the project directory
#[cfg(any(
    all(
        unix,
        not(any(target_os = "android", target_os = "androideabi", target_os = "ios"))
    ),
    windows
))]
#[inline]
pub fn project_dir() -> R<Dirs> {
    let dirs = directories::ProjectDirs::from("net", "MaidSafe", "quic-p2p")
        .ok_or_else(|| QuicP2pError::Io(::std::io::ErrorKind::NotFound.into()))?;
    Ok(Dirs::Desktop(dirs))
}

/// Get the project directory
#[cfg(not(any(
    all(
        unix,
        not(any(target_os = "android", target_os = "androideabi", target_os = "ios"))
    ),
    windows
)))]
#[inline]
pub fn project_dir() -> R<Dirs> {
    Err(QuicP2pError::Configuration {
        e: "No default project dir on non-desktop platforms. User must provide an override path."
            .to_string(),
    })
}

/// Convert binary data to a diplay-able format
#[inline]
pub fn bin_data_format(data: &[u8]) -> String {
    let len = data.len();
    if len < 8 {
        return format!("[ {:?} ]", data);
    }

    format!(
        "[ {:02x} {:02x} {:02x} {:02x}..{:02x} {:02x} {:02x} {:02x} ]",
        data[0],
        data[1],
        data[2],
        data[3],
        data[len - 4],
        data[len - 3],
        data[len - 2],
        data[len - 1]
    )
}

/// Handle error in communication.
#[inline]
pub fn handle_communication_err(
    peer_addr: SocketAddr,
    e: &QuicP2pError,
    details: &str,
    unsent_user_msg: Option<(Peer, bytes::Bytes, Token)>,
) {
    debug!(
        "ERROR in communication with peer {}: {:?} - {}. Details: {}",
        peer_addr, e, e, details
    );
    ctx_mut(|c| {
        let _ = c.connections.remove(&peer_addr);
        if let Some((peer, msg, token)) = unsent_user_msg {
            let _ = c
                .event_tx
                .send(Event::UnsentUserMessage { peer, msg, token });
        }
    });
}

/// Handle successful sends. Currently a NoOp for non-user-messages (i.e., internal messages).
#[inline]
pub fn handle_send_success(sent_user_msg: Option<(Peer, bytes::Bytes, Token)>) {
    ctx_mut(|c| {
        if let Some((peer, msg, token)) = sent_user_msg {
            let _ = c.event_tx.send(Event::SentUserMessage { peer, msg, token });
        }
    });
}

/// Try reading from the disk into the given structure.
pub fn read_from_disk<D>(file_path: &Path) -> R<D>
where
    D: DeserializeOwned,
{
    Ok(File::open(file_path)
        .map_err(Into::into)
        .map(BufReader::new)
        .and_then(|mut rdr| bincode::deserialize_from(&mut rdr))?)
}

/// Try writing the given structure to the disk.
pub fn write_to_disk<S>(file_path: &Path, s: &S) -> R<()>
where
    S: Serialize,
{
    File::create(file_path)
        .map_err(Into::into)
        .map(BufWriter::new)
        .and_then(|mut rdr| bincode::serialize_into(&mut rdr, s))?;

    Ok(())
}
