//! A node's files: its secret key and the blocks it has seen.
use olc_core::Block;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

pub struct Store {
    blocks: File,
    written: u64,
    /// Bytes this store may hold, for a node given a fixed amount of disk.
    limit: Option<u64>,
    full: bool,
}

impl Store {
    pub fn open(directory: &Path) -> Result<(Self, Vec<Block>), String> {
        fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        let path = directory.join("blocks.log");
        let saved = read_blocks(&path);
        let blocks = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|error| error.to_string())?;
        let written = fs::metadata(&path).map_or(0, |file| file.len());
        let store = Self {
            blocks,
            written,
            limit: None,
            full: false,
        };
        Ok((store, saved))
    }

    pub fn set_limit(&mut self, limit: Option<u64>) {
        self.limit = limit;
    }

    pub fn written(&self) -> u64 {
        self.written
    }

    /// True once a block did not fit. The node runs on, and that block lasts only until it stops.
    pub fn is_full(&self) -> bool {
        self.full
    }

    pub fn append(&mut self, block: &Block) {
        let Ok(bytes) = postcard::to_allocvec(block) else {
            return;
        };
        let size = bytes.len() as u64 + 4;
        if self.limit.is_some_and(|limit| self.written + size > limit) {
            self.full = true;
            return;
        }
        let _ = self.blocks.write_all(&(bytes.len() as u32).to_le_bytes());
        let _ = self.blocks.write_all(&bytes);
        self.written += size;
    }
}

fn read_blocks(path: &PathBuf) -> Vec<Block> {
    let mut data = Vec::new();
    if File::open(path)
        .and_then(|mut file| file.read_to_end(&mut data))
        .is_err()
    {
        return Vec::new();
    }
    let mut blocks = Vec::new();
    let mut rest = data.as_slice();
    while rest.len() >= 4 {
        let size = u32::from_le_bytes([rest[0], rest[1], rest[2], rest[3]]) as usize;
        let Some(bytes) = rest.get(4..4 + size) else {
            break;
        };
        match postcard::from_bytes(bytes) {
            Ok(block) => blocks.push(block),
            Err(_) => break,
        }
        rest = &rest[4 + size..];
    }
    blocks
}

/// Reads the secret key, creating one the first time. Only the owner can read the file.
pub fn load_or_create_secret(directory: &Path) -> Result<[u8; 32], String> {
    let path = directory.join("wallet.key");
    if let Ok(text) = fs::read_to_string(&path) {
        return parse_secret(text.trim()).ok_or(format!("{}: bad key", path.display()));
    }
    let mut secret = [0u8; 32];
    getrandom::fill(&mut secret).map_err(|error| error.to_string())?;
    let text: String = secret.iter().map(|byte| format!("{byte:02x}")).collect();
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&path)
        .and_then(|mut file| file.write_all(text.as_bytes()))
        .map_err(|error| error.to_string())?;
    Ok(secret)
}

fn parse_secret(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 || !text.is_ascii() {
        return None;
    }
    let mut secret = [0u8; 32];
    for (index, byte) in secret.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(secret)
}
