// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

#[cfg(any(
    all(
        unix,
        not(any(target_os = "android", target_os = "androideabi", target_os = "ios"))
    ),
    windows
))]
use directories::ProjectDirs;
use std::path::{Path, PathBuf};

/// For Operating Systems beyond Windows or UNIX. May also be used for testing
/// and mobile platforms.
pub struct OverRide {
    path: PathBuf,
}

impl OverRide {
    /// This path will be the root directory that all files will be read and written.
    #[allow(unused)]
    pub fn new(path: &str) -> Self {
        Self {
            path: PathBuf::from(path.to_owned()),
        }
    }
}

/// Location at which data such as config, keys, certificates and other backup data
/// are stored.
pub enum Dirs {
    /// User defined custom directory for all data.
    #[allow(unused)]
    Overide(OverRide),
    /// Set of directories to specifically store the cache, config, app data etc.
    #[cfg(any(
        all(
            unix,
            not(any(target_os = "android", target_os = "androideabi", target_os = "ios"))
        ),
        windows
    ))]
    Desktop(ProjectDirs),
}

impl Dirs {
    /// Location of config, keys and certificates.
    pub(crate) fn config_dir(&self) -> &Path {
        use Dirs::*;
        match *self {
            Overide(ref x) => x.path.as_path(),
            #[cfg(any(
                all(
                    unix,
                    not(any(target_os = "android", target_os = "androideabi", target_os = "ios"))
                ),
                windows
            ))]
            Desktop(ref x) => x.config_dir(),
        }
    }

    /// Location were boostrap cache is stored.
    pub(crate) fn cache_dir(&self) -> &Path {
        use Dirs::*;
        match *self {
            Overide(ref x) => x.path.as_path(),
            #[cfg(any(
                all(
                    unix,
                    not(any(target_os = "android", target_os = "androideabi", target_os = "ios"))
                ),
                windows
            ))]
            Desktop(ref x) => x.cache_dir(),
        }
    }

    /// Location of any backup data for restarts.
    #[allow(unused)]
    pub(crate) fn data_dir(&self) -> &Path {
        use Dirs::*;
        match *self {
            Overide(ref x) => x.path.as_path(),
            #[cfg(any(
                all(
                    unix,
                    not(any(target_os = "android", target_os = "androideabi", target_os = "ios"))
                ),
                windows
            ))]
            Desktop(ref x) => x.data_dir(),
        }
    }
}
