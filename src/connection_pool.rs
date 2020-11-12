// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use slotmap::{DefaultKey, DenseSlotMap};
use std::{
    collections::{hash_map::Entry, HashMap},
    net::SocketAddr,
    sync::{Arc, RwLock},
};

#[derive(Clone)]
pub(crate) struct ConnectionPool {
    store: Arc<RwLock<Store>>,
}

impl ConnectionPool {
    pub fn new() -> Self {
        Self {
            store: Arc::new(RwLock::new(Store {
                connections: DenseSlotMap::new(),
                keys: HashMap::new(),
            })),
        }
    }

    pub fn insert(&self, conn: quinn::Connection) -> Handle {
        let addr = conn.remote_address();

        let mut store = self.store.write().expect("RwLock poisoned");
        let key = store.connections.insert(conn);
        let _ = store.keys.insert(addr, key);

        Handle {
            store: self.store.clone(),
            key,
        }
    }

    pub fn get(&self, addr: &SocketAddr) -> Option<quinn::Connection> {
        let store = self.store.read().ok()?;
        let key = store.keys.get(addr)?;
        store.connections.get(*key).cloned()
    }
}

pub(crate) struct Handle {
    store: Arc<RwLock<Store>>,
    key: DefaultKey,
}

impl Drop for Handle {
    fn drop(&mut self) {
        let mut store = if let Ok(store) = self.store.write() {
            store
        } else {
            return;
        };

        if let Some(conn) = store.connections.remove(self.key) {
            if let Entry::Occupied(entry) = store.keys.entry(conn.remote_address()) {
                if entry.get() == &self.key {
                    let _ = entry.remove();
                }
            }
        }
    }
}

struct Store {
    connections: DenseSlotMap<DefaultKey, quinn::Connection>,
    keys: HashMap<SocketAddr, DefaultKey>,
}
