// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::dirs::Dirs;
use crate::utils;
use crate::{QuicP2pError, R};
use log::info;
use std::{
    collections::{HashSet, VecDeque},
    fs, io,
    net::SocketAddr,
    path::PathBuf,
};

/// Maximum peers in the cache.
const MAX_CACHE_SIZE: usize = 200;

/// A very simple LRU like struct that writes itself to disk every 10 entries added.
pub struct BootstrapCache {
    peers: VecDeque<SocketAddr>,
    cache_path: PathBuf,
    add_count: u8,
    hard_coded_contacts: HashSet<SocketAddr>,
}

impl BootstrapCache {
    /// Tries to construct bootstrap cache backed by a file.
    /// Fails if not able to obtain write permissions for a cache file.
    ///
    /// ## Args
    ///
    /// - hard_coded_contacts: these peers are hard coded into the binary and should
    ///   not be cached upon successful connection.
    pub fn new(
        hard_coded_contacts: HashSet<SocketAddr>,
        user_override: Option<&Dirs>,
    ) -> R<BootstrapCache> {
        let path = |dir: &Dirs| {
            let path = dir.cache_dir();
            path.join("bootstrap_cache")
        };

        let cache_path = user_override.map_or_else(
            || Ok::<_, QuicP2pError>(path(&utils::project_dir()?)),
            |d| Ok(path(d)),
        )?;

        let peers = if cache_path.exists() {
            utils::read_from_disk(&cache_path)?
        } else {
            let cache_dir = cache_path
                .parent()
                .ok_or_else(|| io::ErrorKind::NotFound.into())
                .map_err(QuicP2pError::Io)?;
            fs::create_dir_all(&cache_dir)?;
            Default::default()
        };

        Ok(BootstrapCache {
            peers,
            cache_path,
            add_count: 0u8,
            hard_coded_contacts,
        })
    }

    pub fn peers_mut(&mut self) -> &mut VecDeque<SocketAddr> {
        &mut self.peers
    }

    pub fn peers(&self) -> &VecDeque<SocketAddr> {
        &self.peers
    }

    pub fn hard_coded_contacts(&self) -> &HashSet<SocketAddr> {
        &self.hard_coded_contacts
    }

    /// Caches given peer if it's not in hard coded contacts.
    pub fn add_peer(&mut self, peer: SocketAddr) {
        if self.hard_coded_contacts.contains(&peer) {
            return;
        }

        if self.peers.contains(&peer) {
            self.move_to_cache_top(peer);
        } else {
            self.insert_new(peer);
        }
    }

    fn insert_new(&mut self, peer: SocketAddr) {
        self.peers.push_back(peer);
        self.add_count += 1;
        if self.peers.len() > MAX_CACHE_SIZE {
            let _ = self.peers.pop_front();
        }
        self.try_sync_to_disk();
    }

    fn move_to_cache_top(&mut self, peer: SocketAddr) {
        if let Some(pos) = self.peers.iter().position(|p| *p == peer) {
            let _ = self.peers.remove(pos);
            self.peers.push_back(peer);
        }
    }

    /// Write cached peers to disk every 10 inserted peers.
    fn try_sync_to_disk(&mut self) {
        if self.add_count > 9 {
            if let Err(e) = utils::write_to_disk(&self.cache_path, &self.peers) {
                info!("Failed to write bootstrap cache to disk: {}", e);
            }
            self.add_count = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{make_node_addr, rand_node_addr, test_dirs};
    use unwrap::unwrap;

    mod add_peer {
        use super::*;

        #[test]
        fn when_10_peers_are_added_they_are_synced_to_disk() {
            let dirs = test_dirs();
            let mut cache = unwrap!(BootstrapCache::new(Default::default(), Some(&dirs)));

            for _ in 0..10 {
                cache.add_peer(rand_node_addr());
            }

            assert_eq!(cache.peers.len(), 10);

            let cache = unwrap!(BootstrapCache::new(Default::default(), Some(&dirs)));
            assert_eq!(cache.peers.len(), 10);
        }

        #[test]
        fn when_given_peer_is_in_hard_coded_contacts_it_is_not_cached() {
            let peer1 = rand_node_addr();
            let peer2 = rand_node_addr();
            let mut hard_coded: HashSet<_> = Default::default();
            assert!(hard_coded.insert(peer1.clone()));

            let dirs = test_dirs();
            let mut cache = unwrap!(BootstrapCache::new(hard_coded, Some(&dirs)));

            cache.add_peer(peer1);
            cache.add_peer(peer2.clone());

            let peers: Vec<_> = cache.peers.iter().cloned().collect();
            assert_eq!(peers, vec![peer2]);
        }

        #[test]
        fn it_caps_cache_size() {
            let dirs = test_dirs();
            let port_base = 5000;

            let mut cache = unwrap!(BootstrapCache::new(Default::default(), Some(&dirs)));

            for i in 0..MAX_CACHE_SIZE {
                cache.add_peer(make_node_addr(port_base + i as u16));
            }
            assert_eq!(cache.peers.len(), MAX_CACHE_SIZE);

            cache.add_peer(make_node_addr(port_base + MAX_CACHE_SIZE as u16));
            assert_eq!(cache.peers.len(), MAX_CACHE_SIZE);
        }
    }

    mod move_to_cache_top {
        use super::*;

        #[test]
        fn it_moves_given_node_to_the_top_of_the_list() {
            let dirs = test_dirs();
            let mut cache = unwrap!(BootstrapCache::new(Default::default(), Some(&dirs)));
            let peer1 = rand_node_addr();
            let peer2 = rand_node_addr();
            let peer3 = rand_node_addr();
            cache.add_peer(peer1.clone());
            cache.add_peer(peer2.clone());
            cache.add_peer(peer3.clone());

            cache.move_to_cache_top(peer2.clone());

            let peers: Vec<_> = cache.peers.iter().cloned().collect();
            assert_eq!(peers, vec![peer1, peer3, peer2]);
        }
    }
}
