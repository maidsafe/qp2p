#![allow(unused)]

use directories::ProjectDirs;
use std::path::{Path, PathBuf};

/// For Operating Systems beyond Windows or UNIX. May also be used for testing
/// and mobile platforms.
pub struct OverRide {
    path: PathBuf,
}

impl OverRide {
    /// This path will be the root directory that all files will be read and written.
    pub fn new(path: &str) -> Self {
        Self {
            path: PathBuf::from(path.to_owned()),
        }
    }
}

pub enum Dirs {
    Overide(OverRide),
    #[cfg(not(any(target_os = "android", target_os = "androideabi", target_os = "ios")))]
    Desktop(ProjectDirs),
}

impl Dirs {
    /// Location of config, keys and certificates.
    pub(crate) fn config_dir(&self) -> &Path {
        use Dirs::*;
        match *self {
            Overide(ref x) => x.path.as_path(),
            #[cfg(not(any(target_os = "android", target_os = "androideabi", target_os = "ios")))]
            Desktop(ref x) => x.config_dir(),
        }
    }

    /// Location were boostrap cache is stored.
    pub(crate) fn cache_dir(&self) -> &Path {
        use Dirs::*;
        match *self {
            Overide(ref x) => x.path.as_path(),
            #[cfg(not(any(target_os = "android", target_os = "androideabi", target_os = "ios")))]
            Desktop(ref x) => x.cache_dir(),
        }
    }

    /// Location of any backup data for restarts.
    pub(crate) fn data_dir(&self) -> &Path {
        use Dirs::*;
        match *self {
            Overide(ref x) => x.path.as_path(),
            #[cfg(not(any(target_os = "android", target_os = "androideabi", target_os = "ios")))]
            Desktop(ref x) => x.data_dir(),
        }
    }
}
