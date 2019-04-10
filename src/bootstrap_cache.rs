use crate::dirs::Dirs;
use crate::{Error, NodeInfo, R};
use bincode::{deserialize_from, serialize_into};
use log::{error, info};
use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter};
use std::path::Path;
use std::path::PathBuf;

/// Maximum peers in the cache.
const MAX_CACHE_SIZE: usize = 200;

/// A very simple LRU like struct that writes itself to disk every 10 entries added.
pub struct BootstrapCache {
    peers: VecDeque<NodeInfo>,
    path_buf: PathBuf,
    add_count: u8,
}

impl BootstrapCache {
    pub fn try_new(dirs: &Dirs) -> R<BootstrapCache> {
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
        })
    }

    #[allow(unused)]
    pub fn peers(&self) -> &VecDeque<NodeInfo> {
        &self.peers
    }

    pub fn add_peer(&mut self, peer: NodeInfo) {
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
        .map(|f| BufReader::new(f))
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
        .map(|f| BufWriter::new(f))
        .and_then(|mut rdr| serialize_into(&mut rdr, &data))
        .map_err(|e| {
            error!("could not serialise {}: {}", filename.display(), e);
            e.into()
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dirs::OverRide;
    use rand::Rng;
    use std::env;
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

    fn rand_peer() -> NodeInfo {
        let peer_cert_der =
            rcgen::generate_simple_self_signed(vec!["Test".to_string()]).serialize_der();
        let mut rng = rand::thread_rng();
        let port: u16 = rng.gen();
        let peer_addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), port));
        NodeInfo {
            peer_addr,
            peer_cert_der,
        }
    }

    /// Constructs random bootstrap cache file name.
    fn tmp_dir() -> PathBuf {
        let fname = format!("{:016x}.quic_p2p_tests", rand::random::<u64>());
        let mut path = env::temp_dir();
        path.push(fname);
        path
    }

    fn test_dirs() -> Dirs {
        Dirs::Overide(OverRide::new(&unwrap!(tmp_dir().to_str())))
    }

    #[test]
    fn add_10_close_read_again() {
        let dirs = test_dirs();
        let mut cache = unwrap!(BootstrapCache::try_new(&dirs));

        for _ in 0..10 {
            cache.add_peer(rand_peer());
        }

        assert_eq!(cache.peers().len(), 10);

        let cache = unwrap!(BootstrapCache::try_new(&dirs));
        assert_eq!(cache.peers().len(), 10);
    }

    mod move_to_cache_top {
        use super::*;

        #[test]
        fn it_moves_given_node_to_the_top_of_the_list() {
            let dirs = test_dirs();
            let mut cache = unwrap!(BootstrapCache::try_new(&dirs));
            let peer1 = rand_peer();
            let peer2 = rand_peer();
            let peer3 = rand_peer();
            cache.add_peer(peer1.clone());
            cache.add_peer(peer2.clone());
            cache.add_peer(peer3.clone());

            cache.move_to_cache_top(peer2.clone());

            let peers: Vec<NodeInfo> = cache.peers().iter().cloned().collect();
            assert_eq!(peers, vec![peer1, peer3, peer2]);
        }
    }
}
