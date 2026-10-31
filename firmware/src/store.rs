//! Flash storage: the wallet key, the network settings and a log of every block, so a board
//! survives losing power.
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use esp_storage::FlashStorage;
use olc_core::{Block, Hash};

/// Past the end of the 4 MB the app can use, on a board with 16 MB of flash.
const META: u32 = 0x40_0000;
const SETTINGS: u32 = META + 0x1000;
const LOG_START: u32 = META + 0x2000;
const LOG_END: u32 = 0x100_0000;
const MAGIC: u32 = 0x4F4C_4331;
const META_LEN: usize = 4 + 32 + 32;
const RECORD_HEADER: usize = 8;
const MAX_RECORD: usize = 64 * 1024;

const SETTINGS_LEN: usize = 256;

/// Set with the `wifi`, `dial`, `view` and `lang` commands. Empty means off or the default.
#[derive(Default)]
pub struct Settings {
    pub ssid: String,
    pub password: String,
    pub dial: String,
    pub view: String,
    pub language: String,
}

pub struct Store {
    flash: FlashStorage<'static>,
    head: u32,
    genesis_matches: bool,
}

fn word(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

impl Store {
    pub fn new(flash: FlashStorage<'static>) -> Self {
        Self {
            flash,
            head: LOG_START,
            genesis_matches: false,
        }
    }

    pub fn settings(&mut self) -> Settings {
        let mut bytes = [0u8; SETTINGS_LEN];
        let _ = self.flash.read(SETTINGS, &mut bytes);
        if word(&bytes) != MAGIC {
            return Settings::default();
        }
        let size = usize::min(word(&bytes[4..]) as usize, SETTINGS_LEN - RECORD_HEADER);
        let text = core::str::from_utf8(&bytes[RECORD_HEADER..][..size]).unwrap_or("");
        let mut lines = text.split('\n').map(String::from);
        Settings {
            ssid: lines.next().unwrap_or_default(),
            password: lines.next().unwrap_or_default(),
            dial: lines.next().unwrap_or_default(),
            view: lines.next().unwrap_or_default(),
            language: lines.next().unwrap_or_default(),
        }
    }

    pub fn save_settings(&mut self, settings: &Settings) {
        let text = alloc::format!(
            "{}\n{}\n{}\n{}\n{}",
            settings.ssid,
            settings.password,
            settings.dial,
            settings.view,
            settings.language,
        );
        let mut bytes = [0u8; SETTINGS_LEN];
        if text.len() + RECORD_HEADER <= SETTINGS_LEN {
            bytes[..4].copy_from_slice(&MAGIC.to_le_bytes());
            bytes[4..8].copy_from_slice(&(text.len() as u32).to_le_bytes());
            bytes[RECORD_HEADER..][..text.len()].copy_from_slice(text.as_bytes());
            let _ = self.flash.write(SETTINGS, &bytes);
        }
    }

    /// Returns the saved key, or saves `create()`. A key outlives a change of coin, a log does not.
    pub fn secret(&mut self, genesis: &Hash, create: impl FnOnce() -> [u8; 32]) -> [u8; 32] {
        let mut meta = [0u8; META_LEN];
        let _ = self.flash.read(META, &mut meta);
        let known = word(&meta) == MAGIC;
        self.genesis_matches = known && meta[4..36] == genesis[..];
        let mut secret = [0u8; 32];
        if known {
            secret.copy_from_slice(&meta[36..]);
        } else {
            secret = create();
        }
        if !self.genesis_matches {
            meta[..4].copy_from_slice(&MAGIC.to_le_bytes());
            meta[4..36].copy_from_slice(genesis);
            meta[36..].copy_from_slice(&secret);
            let _ = self.flash.write(META, &meta);
            let _ = self.flash.write(LOG_START, &[0xFF; 4]);
        }
        secret
    }

    /// Reads the log back and leaves the head after its last whole record.
    pub fn load_blocks(&mut self) -> Vec<Block> {
        let mut blocks = Vec::new();
        if !self.genesis_matches {
            return blocks;
        }
        let mut at = LOG_START;
        loop {
            let mut header = [0u8; RECORD_HEADER];
            if at + RECORD_HEADER as u32 >= LOG_END || self.flash.read(at, &mut header).is_err() {
                break;
            }
            let size = word(&header[4..]) as usize;
            if word(&header) != MAGIC || size > MAX_RECORD {
                break;
            }
            let mut bytes = vec![0u8; size];
            if self
                .flash
                .read(at + RECORD_HEADER as u32, &mut bytes)
                .is_err()
            {
                break;
            }
            match postcard::from_bytes(&bytes) {
                Ok(block) => blocks.push(block),
                Err(_) => break,
            }
            at += (RECORD_HEADER + size.next_multiple_of(4)) as u32;
        }
        self.head = at;
        blocks
    }

    /// Each record ends with an erased word, which is where the next read stops.
    pub fn append(&mut self, block: &Block) {
        let Ok(bytes) = postcard::to_allocvec(block) else {
            return;
        };
        let body = bytes.len().next_multiple_of(4);
        let mut record = Vec::with_capacity(RECORD_HEADER + body + 4);
        record.extend_from_slice(&MAGIC.to_le_bytes());
        record.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        record.extend_from_slice(&bytes);
        record.resize(RECORD_HEADER + body, 0);
        record.extend_from_slice(&[0xFF; 4]);
        if self.head + record.len() as u32 <= LOG_END
            && self.flash.write(self.head, &record).is_ok()
        {
            self.head += (RECORD_HEADER + body) as u32;
        }
    }
}
