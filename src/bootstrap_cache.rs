// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::dirs::Dirs;
use crate::{Error, NodeInfo, R};
use bincode::{deserialize_from, serialize_into};
use directories::ProjectDirs;
use log::{error, info};
use std::collections::{HashSet, VecDeque};
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter};
use std::path::Path;
use std::path::PathBuf;

/// Maximum peers in the cache.
const MAX_CACHE_SIZE: usize = 200;

/// Tries to determine the best location for bootstrap cache and constructs it.
pub fn init_bootstrap_cache(mut hard_coded_contacts: Vec<NodeInfo>) -> R<BootstrapCache> {
    let dirs = ProjectDirs::from("net", "MaidSafe", "quic-p2p")
        .ok_or_else(|| io::ErrorKind::NotFound.into())
        .map_err(Error::IoError)?;
    let dirs = Dirs::Desktop(dirs);
    // TODO(povilas): update config to use HashSet for hard coded contacts - it's a more
    // appropriate data structure given that we will never have 2 identical contacts.
    let hard_coded_contacts: HashSet<_> = hard_coded_contacts.drain(..).collect();
    BootstrapCache::try_new(&dirs, hard_coded_contacts)
}

/// A very simple LRU like struct that writes itself to disk every 10 entries added.
pub struct BootstrapCache {
    peers: VecDeque<NodeInfo>,
    path_buf: PathBuf,
    add_count: u8,
    hard_coded_contacts: HashSet<NodeInfo>,
}

impl BootstrapCache {
    /// Tries to construct bootstrap cache backed by a file.
    /// Fails if not able to obtain write permissions for a cache file.
    ///
    /// ## Args
    ///
    /// - hard_coded_contacts: these peers are hard coded into the binary and should
    ///   not be cached upon successful connection.
    pub fn try_new(dirs: &Dirs, hard_coded_contacts: HashSet<NodeInfo>) -> R<BootstrapCache> {
        let cache_file = path(dirs);
        let peers = if cache_file.exists() {
            read_from_disk(&cache_file)?
        } else {
            let cache_dir = cache_file
                .parent()
                .ok_or_else(|| io::ErrorKind::NotFound.into())
                .map_err(Error::IoError)?;
            fs::create_dir_all(&cache_dir)?;
            Default::default()
        };

        Ok(BootstrapCache {
            peers,
            path_buf: cache_file,
            add_count: 0u8,
            hard_coded_contacts,
        })
    }

    pub fn peers_mut(&mut self) -> &mut VecDeque<NodeInfo> {
        &mut self.peers
    }

    pub fn peers(&self) -> &VecDeque<NodeInfo> {
        &self.peers
    }

    pub fn hard_coded_contacts(&self) -> &HashSet<NodeInfo> {
        &self.hard_coded_contacts
    }

    /// Caches given peer if it's not in hard coded contacts.
    pub fn add_peer(&mut self, peer: NodeInfo) {
        if self.hard_coded_contacts.contains(&peer) {
            return;
        }

        if self.peers.contains(&peer) {
            self.move_to_cache_top(peer);
        } else {
            self.insert_new(peer);
        }
    }

    fn insert_new(&mut self, peer: NodeInfo) {
        self.peers.push_back(peer);
        self.add_count += 1;
        if self.peers.len() > MAX_CACHE_SIZE {
            let _ = self.peers.pop_front();
        }
        self.try_sync_to_disk();
    }

    fn move_to_cache_top(&mut self, peer: NodeInfo) {
        if let Some(pos) = self.peers.iter().position(|p| *p == peer) {
            let _ = self.peers.remove(pos);
            self.peers.push_back(peer);
        }
    }

    /// Write cached peers to disk every 10 inserted peers.
    fn try_sync_to_disk(&mut self) {
        if self.add_count > 9 {
            if let Err(e) = write_to_disk(&self.path_buf.as_path(), self.peers.clone()) {
                info!("Failed to write bootstrap cache to disk: {}", e);
            }
            self.add_count = 0;
        }
    }
}

/// Returns a path to bootstrap cache file.
fn path(dirs: &Dirs) -> PathBuf {
    let path = dirs.cache_dir();
    path.join("bootstrap_cache")
}

fn read_from_disk(filename: &Path) -> R<VecDeque<NodeInfo>> {
    File::open(filename)
        .map_err(|e| {
            error!("could not open {}: {}", filename.display(), e);
            e.into()
        })
        .map(BufReader::new)
        .and_then(|mut rdr| deserialize_from(&mut rdr))
        .map_err(|e| {
            error!("could not deserialise {}: {}", filename.display(), e);
            e.into()
        })
}

fn write_to_disk(filename: &Path, data: VecDeque<NodeInfo>) -> R<()> {
    File::create(filename)
        .map_err(|e| {
            error!("could not create {}: {}", filename.display(), e);
            e.into()
        })
        .map(BufWriter::new)
        .and_then(|mut rdr| serialize_into(&mut rdr, &data))
        .map_err(|e| {
            error!("could not serialise {}: {}", filename.display(), e);
            e.into()
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::testing::{rand_node_info, test_dirs};
    use speculate::speculate;

    speculate! {
        before {
            let dirs = test_dirs();
            #[allow(unused)]
            let mut cache = unwrap!(BootstrapCache::try_new(&dirs, HashSet::new()));
        }

        describe "add_peer" {
            describe "when 10 peers are added" {
                before {
                    for _ in 0..10 {
                        cache.add_peer(rand_node_info());
                    }
                }

                it "syncs cache to disk" {
                    assert_eq!(cache.peers.len(), 10);

                    let cache = unwrap!(BootstrapCache::try_new(&dirs, HashSet::new()));
                    assert_eq!(cache.peers.len(), 10);
                }
            }

            describe "when given peer is in hard coded contacts" {
                it "does not cache this peer" {
                    let peer1 = rand_node_info();
                    let peer2 = rand_node_info();
                    let hard_coded = vec![peer1.clone()].iter().cloned().collect();

                    let mut cache = unwrap!(BootstrapCache::try_new(&dirs, hard_coded));

                    cache.add_peer(peer1.clone());
                    cache.add_peer(peer2.clone());

                    let peers: Vec<NodeInfo> = cache.peers.iter().cloned().collect();
                    assert_eq!(peers, vec![peer2]);
                }
            }

            it "caps cache size" {
                for _ in 0..MAX_CACHE_SIZE {
                    cache.add_peer(rand_node_info());
                }
                assert_eq!(cache.peers.len(), MAX_CACHE_SIZE);

                cache.add_peer(rand_node_info());
                assert_eq!(cache.peers.len(), MAX_CACHE_SIZE);
            }
        }

        describe "move_to_cache_top" {
            it "moves given node to the top of the list" {
                let peer1 = rand_node_info();
                let peer2 = rand_node_info();
                let peer3 = rand_node_info();
                cache.add_peer(peer1.clone());
                cache.add_peer(peer2.clone());
                cache.add_peer(peer3.clone());

                cache.move_to_cache_top(peer2.clone());

                let peers: Vec<NodeInfo> = cache.peers.iter().cloned().collect();
                assert_eq!(peers, vec![peer1, peer3, peer2]);
            }
        }
    }
}
