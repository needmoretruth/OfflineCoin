//! A channel node's files. Each channel is written whole, flushed to the disk and moved into
//! place before anything it led to is sent, so a cut at any moment leaves either the step before
//! or the step after, and never a revoked state taken for the latest.
use crate::channel::Channel;
use crate::wire::ChannelId;
use crate::bytes::hex;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

const KEY: &str = "node.key";
const BOOK: &str = "book.json";
const CHANNEL_START: &str = "channel-";

pub struct Store {
    folder: PathBuf,
}

/// Writes `bytes` in place of `file` so that a cut leaves the old file or the new one.
pub fn write_through(file: &Path, bytes: &[u8]) -> Result<(), String> {
    let fresh = file.with_extension("new");
    let mut out = File::create(&fresh).map_err(|error| error.to_string())?;
    out.write_all(bytes).map_err(|error| error.to_string())?;
    out.sync_all().map_err(|error| error.to_string())?;
    drop(out);
    fs::rename(&fresh, file).map_err(|error| error.to_string())?;
    if let Some(folder) = file.parent() {
        // The move itself is safe once the folder is on the disk too.
        if let Ok(folder) = File::open(folder) {
            let _ = folder.sync_all();
        }
    }
    Ok(())
}

#[cfg(unix)]
fn keep_private(file: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(file, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn keep_private(_file: &Path) {}

impl Store {
    pub fn open(folder: &Path) -> Result<Self, String> {
        fs::create_dir_all(folder).map_err(|error| error.to_string())?;
        Ok(Self { folder: folder.to_path_buf() })
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }

    /// This node's secret key, made the first time.
    pub fn key(&self) -> Result<[u8; 32], String> {
        let file = self.folder.join(KEY);
        if let Ok(bytes) = fs::read(&file) {
            return bytes.try_into().map_err(|_| format!("{} is damaged", file.display()));
        }
        let key = fresh_secret()?;
        write_through(&file, &key)?;
        keep_private(&file);
        Ok(key)
    }

    fn channel_file(&self, id: &ChannelId) -> PathBuf {
        self.folder.join(format!("{CHANNEL_START}{}.json", hex(id)))
    }

    pub fn save_channel(&self, channel: &Channel) -> Result<(), String> {
        let file = self.channel_file(&channel.id);
        write_through(&file, &to_json(channel)?)?;
        keep_private(&file);
        Ok(())
    }

    pub fn channels(&self) -> Vec<Channel> {
        let Ok(entries) = fs::read_dir(&self.folder) else {
            return Vec::new();
        };
        let mut found: Vec<Channel> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
                name.starts_with(CHANNEL_START) && name.ends_with(".json")
            })
            .filter_map(|path| read_json(&path))
            .collect();
        found.sort_by_key(|channel| channel.made_ms);
        found
    }

    pub fn book<T: DeserializeOwned + Default>(&self) -> T {
        read_json(&self.folder.join(BOOK)).unwrap_or_default()
    }

    pub fn save_book<T: Serialize>(&self, book: &T) -> Result<(), String> {
        write_through(&self.folder.join(BOOK), &to_json(book)?)
    }
}

fn to_json<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    serde_json::to_vec(value).map_err(|error| error.to_string())
}

fn read_json<T: DeserializeOwned>(file: &Path) -> Option<T> {
    serde_json::from_slice(&fs::read(file).ok()?).ok()
}

/// 32 bytes from the system's random source.
pub fn fresh_secret() -> Result<[u8; 32], String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|error| error.to_string())?;
    Ok(bytes)
}
