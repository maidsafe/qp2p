// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use std::{collections::BTreeMap, net::SocketAddr, sync::Arc};

use tiny_keccak::{Hasher, Sha3};
use tokio::sync::RwLock;
use xor_name::XorName;

// Pool for keeping open connections. Pooled connections are associated with a `ConnectionRemover`
// which can be used to remove them from the pool.
#[derive(Clone)]
pub(crate) struct ConnectionPool<I: ConnId> {
    store: Arc<RwLock<Store<I>>>,
}

impl<I: ConnId> ConnectionPool<I> {
    pub fn new() -> Self {
        Self {
            store: Arc::new(RwLock::new(Store::default())),
        }
    }

    pub async fn insert(
        &self,
        id: I,
        addr: SocketAddr,
        conn: quinn::Connection,
    ) -> ConnectionRemover<I> {
        let mut store = self.store.write().await;

        let key = Key {
            addr,
            id: store.id_gen.next(),
        };
        let _ = store.id_map.insert(id, (conn.clone(), key));
        let _ = store.key_map.insert(key, (conn, id));

        ConnectionRemover {
            store: self.store.clone(),
            id,
            key,
        }
    }

    pub async fn has_addr(&self, addr: &SocketAddr) -> bool {
        let store = self.store.read().await;

        // Efficiently fetch the first entry whose key is equal to `key` and check if it exists
        store
            .key_map
            .range(Key::min(*addr)..=Key::max(*addr))
            .next()
            .is_some()
    }

    #[allow(unused)]
    pub async fn has_id(&self, id: &I) -> bool {
        let store = self.store.read().await;

        store.id_map.contains_key(id)
    }

    pub async fn remove(&self, addr: &SocketAddr) -> Vec<quinn::Connection> {
        let mut store = self.store.write().await;

        let keys_to_remove = store
            .key_map
            .range_mut(Key::min(*addr)..=Key::max(*addr))
            .into_iter()
            .map(|(key, _)| *key)
            .collect::<Vec<_>>();

        keys_to_remove
            .iter()
            .filter_map(|key| store.key_map.remove(key).map(|entry| entry.0))
            .collect::<Vec<_>>()
    }

    pub async fn get_by_id(&self, addr: &I) -> Option<(quinn::Connection, ConnectionRemover<I>)> {
        let store = self.store.read().await;

        let (conn, key) = store.id_map.get(addr)?;

        let remover = ConnectionRemover {
            store: self.store.clone(),
            key: *key,
            id: *addr,
        };

        Some((conn.clone(), remover))
    }

    pub async fn get_by_addr(
        &self,
        addr: &SocketAddr,
    ) -> Option<(quinn::Connection, ConnectionRemover<I>)> {
        let store = self.store.read().await;

        // Efficiently fetch the first entry whose key is equal to `key`.
        let (key, entry) = store
            .key_map
            .range(Key::min(*addr)..=Key::max(*addr))
            .next()?;

        let conn = entry.clone().0;
        let remover = ConnectionRemover {
            store: self.store.clone(),
            key: *key,
            id: entry.1,
        };

        Some((conn, remover))
    }
}

// Handle for removing a connection from the pool.
#[derive(Clone)]
pub(crate) struct ConnectionRemover<I: ConnId> {
    store: Arc<RwLock<Store<I>>>,
    key: Key,
    id: I,
}

impl<I: ConnId> ConnectionRemover<I> {
    // Remove the connection from the pool.
    pub async fn remove(&self) {
        let mut store = self.store.write().await;
        let _ = store.key_map.remove(&self.key);
        let _ = store.id_map.remove(&self.id);
    }

    pub fn remote_addr(&self) -> &SocketAddr {
        &self.key.addr
    }

    pub fn id(&self) -> I {
        self.id
    }
}

#[derive(Default)]
struct Store<I: ConnId> {
    id_map: BTreeMap<I, (quinn::Connection, Key)>,
    key_map: BTreeMap<Key, (quinn::Connection, I)>,
    id_gen: IdGen,
}

/// Unique key identifying a connection. Two connections will always have distict keys even if they
/// have the same socket address.
pub trait ConnId:
    Clone + Copy + Eq + PartialEq + Ord + PartialOrd + Default + Send + Sync + 'static
{
    /// Generate
    fn generate(socket_addr: &SocketAddr) -> Result<Self, Box<dyn std::error::Error>>;
}

impl ConnId for XorName {
    fn generate(addr: &SocketAddr) -> Result<Self, Box<dyn std::error::Error>> {
        let data = bincode::serialize(addr)?;
        let mut hasher = Sha3::v256();
        let mut output = [0u8; 32];
        hasher.update(&data);
        hasher.finalize(&mut output);
        Ok(XorName(output))
    }
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
