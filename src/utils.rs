// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::error::{Error, Result};
use bincode::{deserialize_from, serialize_into};
use dirs_next::home_dir;
use serde::{de::DeserializeOwned, Serialize};
use std::{
    fs::File,
    io::{BufReader, BufWriter},
    path::{Path, PathBuf},
};

/// Get the project directory
#[cfg(any(
    all(
        unix,
        not(any(target_os = "android", target_os = "androideabi", target_os = "ios"))
    ),
    windows
))]
#[inline]
pub fn project_dir() -> Result<PathBuf> {
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
pub fn project_dir() -> Result<Dirs> {
    Err(Error::Configuration {
        e: "No default project dir on non-desktop platforms. User must provide an override path."
            .to_string(),
    })
}

/// Convert binary data to a diplay-able format
#[inline]
pub fn bin_data_format(data: &[u8]) -> String {
    let len = data.len();
    if len < 8 {
        return format!("[ {:?} ]", data);
    }

    format!(
        "[ {:02x} {:02x} {:02x} {:02x}..{:02x} {:02x} {:02x} {:02x} ]",
        data[0],
        data[1],
        data[2],
        data[3],
        data[len - 4],
        data[len - 3],
        data[len - 2],
        data[len - 1]
    )
}

/// Try reading from the disk into the given structure.
pub fn read_from_disk<D>(file_path: &Path) -> Result<D>
where
    D: DeserializeOwned,
{
    Ok(File::open(file_path)
        .map_err(Into::into)
        .map(BufReader::new)
        .and_then(|mut rdr| deserialize_from(&mut rdr))?)
}

/// Try writing the given structure to the disk.
pub fn write_to_disk<S>(file_path: &Path, s: &S) -> Result<()>
where
    S: Serialize,
{
    File::create(file_path)
        .map_err(Into::into)
        .map(BufWriter::new)
        .and_then(|mut rdr| serialize_into(&mut rdr, s))?;

    Ok(())
}

// #[cfg(test)]
pub(crate) fn init_logging() {
    use flexi_logger::{DeferredNow, Logger};
    use log::Record;
    use std::io::Write;

    // Custom formatter for logs
    let do_format = move |writer: &mut dyn Write, clock: &mut DeferredNow, record: &Record| {
        let handle = std::thread::current();
        write!(
            writer,
            "[{}] {} {} [{}:{}] {}",
            handle
                .name()
                .unwrap_or(&format!("Thread-{:?}", handle.id())),
            record.level(),
            clock.now().to_rfc3339(),
            record.file().unwrap_or_default(),
            record.line().unwrap_or_default(),
            record.args()
        )
    };

    Logger::with_env()
        .format(do_format)
        .suppress_timestamp()
        .start()
        .map(|_| ())
        .unwrap_or(());
}
