// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

#![allow(unused)]

use crate::error::{Error, Result};
use bincode::{deserialize_from, serialize_into};
use dirs_next::home_dir;
use log::warn;
use serde::{de::DeserializeOwned, Serialize};
use std::{
    collections::{HashSet, VecDeque},
    fs,
    net::SocketAddr,
    path::{Path, PathBuf},
};
use std::{
    fs::File,
    io::{BufReader, BufWriter},
};

/// Maximum peers in the cache.
const MAX_CACHE_SIZE: usize = 200;

/// A very simple LRU like struct that writes itself to disk every 10 entries added.
#[derive(Debug, Clone)]
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
        user_override: Option<&PathBuf>,
    ) -> Result<BootstrapCache> {
        let get_cache_path = |dir: &PathBuf| dir.join("bootstrap_cache");

        let cache_path = user_override.map_or_else(
            || Ok::<_, Error>(get_cache_path(&project_dir()?)),
            |d| Ok(get_cache_path(d)),
        )?;

        let peers = if cache_path.exists() {
            read_from_disk(&cache_path)?
        } else {
            let cache_dir = cache_path.parent().ok_or_else(|| {
                Error::InvalidPath(format!(
                    "Failed to get parent directory of '{}'",
                    cache_path.display().to_string()
                ))
            })?;
            fs::create_dir_all(&cache_dir)?;

            VecDeque::new()
        };

        Ok(BootstrapCache {
            peers,
            cache_path,
            add_count: 0_u8,
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

    pub fn clear_from_disk(user_override: Option<&PathBuf>) -> Result<()> {
        let get_cache_path = |dir: &PathBuf| dir.join("bootstrap_cache");

        let cache_path = user_override.map_or_else(
            || Ok::<_, Error>(get_cache_path(&project_dir()?)),
            |d| Ok(get_cache_path(d)),
        )?;

        if cache_path.exists() {
            std::fs::remove_file(cache_path)?;
        }
        Ok(())
    }

    fn move_to_cache_top(&mut self, peer: SocketAddr) {
        if let Some(pos) = self.peers.iter().position(|p| *p == peer) {
            let _ = self.peers.remove(pos);
            self.peers.push_back(peer);
        }
    }

    /// Write cached peers to disk every 10 inserted peers.
    pub(crate) fn try_sync_to_disk(&mut self) {
        if self.add_count > 9 {
            if let Err(e) = write_to_disk(&self.cache_path, &self.peers) {
                warn!("Failed to write bootstrap cache to disk: {}", e);
            }
            self.add_count = 0;
        }
    }
}

// Private utilities/helpers below

/// Get the project directory
#[cfg(any(
    all(
        unix,
        not(any(target_os = "android", target_os = "androideabi", target_os = "ios"))
    ),
    windows
))]
#[inline]
fn project_dir() -> Result<PathBuf> {
    let dirs = home_dir().ok_or(Error::UserHomeDir)?;
    let project_dir = dirs.join(".safe").join("qp2p");
    Ok(project_dir)
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
fn project_dir() -> Result<Dirs> {
    Err(Error::Configuration {
        e: "No default project dir on non-desktop platforms. User must provide an override path."
            .to_string(),
    })
}

/// Try reading from the disk into the given structure.
fn read_from_disk<D>(file_path: &Path) -> Result<D>
where
    D: DeserializeOwned,
{
    Ok(File::open(file_path)
        .map_err(Into::into)
        .map(BufReader::new)
        .and_then(|mut rdr| deserialize_from(&mut rdr))?)
}

/// Try writing the given structure to the disk.
fn write_to_disk<S>(file_path: &Path, s: &S) -> Result<()>
where
    S: Serialize,
{
    File::create(file_path)
        .map_err(Into::into)
        .map(BufWriter::new)
        .and_then(|mut rdr| serialize_into(&mut rdr, s))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{make_node_addr, rand_node_addr, test_dirs};

    mod add_peer {
        use super::*;

        #[test]
        fn when_10_peers_are_added_they_are_synced_to_disk() -> Result<(), Error> {
            let dirs = test_dirs();
            let mut cache = BootstrapCache::new(Default::default(), Some(&dirs))?;

            for _ in 0..10 {
                cache.add_peer(rand_node_addr());
            }

            assert_eq!(cache.peers.len(), 10);

            let cache = BootstrapCache::new(Default::default(), Some(&dirs))?;
            assert_eq!(cache.peers.len(), 10);
            Ok(())
        }

        #[test]
        fn when_given_peer_is_in_hard_coded_contacts_it_is_not_cached() -> Result<(), Error> {
            let peer1 = rand_node_addr();
            let peer2 = rand_node_addr();
            let mut hard_coded: HashSet<_> = Default::default();
            assert!(hard_coded.insert(peer1));

            let dirs = test_dirs();
            let mut cache = BootstrapCache::new(hard_coded, Some(&dirs))?;

            cache.add_peer(peer1);
            cache.add_peer(peer2);

            let peers: Vec<_> = cache.peers.iter().cloned().collect();
            assert_eq!(peers, vec![peer2]);
            Ok(())
        }

        #[test]
        fn it_caps_cache_size() -> Result<(), Error> {
            let dirs = test_dirs();
            let port_base = 5000;

            let mut cache = BootstrapCache::new(Default::default(), Some(&dirs))?;

            for i in 0..MAX_CACHE_SIZE {
                cache.add_peer(make_node_addr(port_base + i as u16));
            }
            assert_eq!(cache.peers.len(), MAX_CACHE_SIZE);

            cache.add_peer(make_node_addr(port_base + MAX_CACHE_SIZE as u16));
            assert_eq!(cache.peers.len(), MAX_CACHE_SIZE);
            Ok(())
        }
    }

    mod move_to_cache_top {
        use super::*;

        #[test]
        fn it_moves_given_node_to_the_top_of_the_list() -> Result<(), Error> {
            let dirs = test_dirs();
            let mut cache = BootstrapCache::new(Default::default(), Some(&dirs))?;
            let peer1 = rand_node_addr();
            let peer2 = rand_node_addr();
            let peer3 = rand_node_addr();
            cache.add_peer(peer1);
            cache.add_peer(peer2);
            cache.add_peer(peer3);

            cache.move_to_cache_top(peer2);

            let peers: Vec<_> = cache.peers.iter().cloned().collect();
            assert_eq!(peers, vec![peer1, peer3, peer2]);
            Ok(())
        }
    }
}
