// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use std::sync::Arc;
use std::{
    collections::{hash_map::Entry, HashMap},
    net::SocketAddr,
};
use thiserror::Error;
use tokio::sync::{broadcast, Mutex};

type Result<T = (), E = Error> = std::result::Result<T, E>;

// Deduplicate multiple concurrent connect attempts to the same peer - they all will yield the
// same `Connection` instead of opening a separate connection each.
#[derive(Clone)]
pub(crate) struct ConnectionDeduplicator {
    map: Arc<Mutex<HashMap<SocketAddr, broadcast::Sender<Result>>>>,
}

impl ConnectionDeduplicator {
    pub fn new() -> Self {
        Self {
            map: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    // Query if there already is a connect attempt to the given address.
    // If this is the first connect attempt, it returns `None` and we should proceed with
    // establishing the connection and then call `complete` with the result.
    // For any subsequent connect attempt this returns `Some` with the result of that attempt.
    pub async fn query(&self, addr: &SocketAddr) -> Option<Result> {
        let mut rx = match self.map.lock().await.entry(*addr) {
            Entry::Occupied(entry) => entry.get().subscribe(),
            Entry::Vacant(entry) => {
                let (tx, _) = broadcast::channel(1);
                let _ = entry.insert(tx);
                return None;
            }
        };

        if let Ok(result) = rx.recv().await {
            Some(result)
        } else {
            // NOTE: this branch cannot realistically happen, because we never drop the `Sender`
            // without sending through it first and we also take it out of the map before doing so
            // which means it's not possible to create more subscription on it after the send.
            // This, however, is not statically provable so we still need to nominally handle this
            // branch. We return `LocallyClosed` which seems like it would be the right error to
            // return if this situation was actually possible (it isn't) as it would signal the
            // caller to either abandon the connect or try to repeat it.
            Some(Err(quinn::ConnectionError::LocallyClosed.into()))
        }
    }

    // Signal completion of a connect attempt. This causes all the pending `query` calls for the
    // same `addr` to return `result`.
    pub async fn complete(&self, addr: &SocketAddr, result: Result) {
        let tx = self.map.lock().await.remove(addr);
        if let Some(tx) = tx {
            let _ = tx.send(result);
        }
    }
}

#[derive(Clone, Error, Debug)]
pub(crate) enum Error {
    #[error("Connect error")]
    Connect(#[from] quinn::ConnectError),
    #[error("Connection error")]
    Connection(#[from] quinn::ConnectionError),
}

impl From<Error> for crate::error::Error {
    fn from(src: Error) -> Self {
        match src {
            Error::Connect(source) => Self::Connect(source),
            Error::Connection(source) => Self::Connection(source),
        }
    }
}
