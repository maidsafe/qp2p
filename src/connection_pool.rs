// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use std::{collections::BTreeMap, net::SocketAddr, sync::Arc};

use tokio::sync::RwLock;

// Pool for keeping open connections. Pooled connections are associated with a `ConnectionRemover`
// which can be used to remove them from the pool.
#[derive(Clone)]
pub(crate) struct ConnectionPool {
    store: Arc<RwLock<Store>>,
}

impl ConnectionPool {
    pub fn new() -> Self {
        Self {
            store: Arc::new(RwLock::new(Store::default())),
        }
    }

    pub async fn insert(&self, addr: SocketAddr, conn: quinn::Connection) -> ConnectionRemover {
        let mut store = self.store.write().await;

        let key = Key {
            addr,
            id: store.id_gen.next(),
        };
        let _ = store.map.insert(key, conn);

        ConnectionRemover {
            store: self.store.clone(),
            key,
        }
    }

    pub async fn has(&self, addr: &SocketAddr) -> bool {
        let store = self.store.read().await;

        // Efficiently fetch the first entry whose key is equal to `key` and check if it exists
        store
            .map
            .range(Key::min(*addr)..=Key::max(*addr))
            .next()
            .is_some()
    }

    pub async fn remove(&self, addr: &SocketAddr) -> Vec<quinn::Connection> {
        let mut store = self.store.write().await;

        let keys_to_remove = store
            .map
            .range_mut(Key::min(*addr)..=Key::max(*addr))
            .into_iter()
            .map(|(key, _)| *key)
            .collect::<Vec<_>>();

        keys_to_remove
            .iter()
            .filter_map(|key| store.map.remove(&key))
            .collect::<Vec<_>>()
    }

    pub async fn get(&self, addr: &SocketAddr) -> Option<(quinn::Connection, ConnectionRemover)> {
        let store = self.store.read().await;

        // Efficiently fetch the first entry whose key is equal to `key`.
        let (key, conn) = store.map.range(Key::min(*addr)..=Key::max(*addr)).next()?;

        let conn = conn.clone();
        let remover = ConnectionRemover {
            store: self.store.clone(),
            key: *key,
        };

        Some((conn, remover))
    }
}

// Handle for removing a connection from the pool.
#[derive(Clone)]
pub(crate) struct ConnectionRemover {
    store: Arc<RwLock<Store>>,
    key: Key,
}

impl ConnectionRemover {
    // Remove the connection from the pool.
    pub async fn remove(&self) {
        let mut store = self.store.write().await;
        let _ = store.map.remove(&self.key);
    }

    pub fn remote_addr(&self) -> &SocketAddr {
        &self.key.addr
    }
}

#[derive(Default)]
struct Store {
    map: BTreeMap<Key, quinn::Connection>,
    id_gen: IdGen,
}

// Unique key identifying a connection. Two connections will always have distict keys even if they
// have the same socket address.
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
struct Key {
    addr: SocketAddr,
    id: u64,
}

impl Key {
    // Returns the minimal `Key` for the given address according to its `Ord` relation.
    fn min(addr: SocketAddr) -> Self {
        Self { addr, id: u64::MIN }
    }

    // Returns the maximal `Key` for the given address according to its `Ord` relation.
    fn max(addr: SocketAddr) -> Self {
        Self { addr, id: u64::MAX }
    }
}

#[derive(Default)]
struct IdGen(u64);

impl IdGen {
    fn next(&mut self) -> u64 {
        let id = self.0;
        self.0 = self.0.wrapping_add(1);
        id
    }
}
