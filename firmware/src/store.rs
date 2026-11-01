//! Flash storage: the wallet key, the network settings and a log of every block, so a board
//! survives losing power.
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use esp_bootloader_esp_idf::ota::OtaImageState;
use esp_bootloader_esp_idf::ota_updater::OtaUpdater;
use esp_bootloader_esp_idf::partitions::{
    read_partition_table, AppPartitionSubType, PartitionType, PARTITION_TABLE_MAX_LEN,
};
use esp_storage::FlashStorage;
use olc_control::bootlog::{self, Stage};
use olc_control::wear::{Flash, Ring};
use olc_core::{Block, Hash};
use olc_crypto::PublicKey;

/// The `chain` partition, past the end of the first app slot. See `partitions.csv`.
const META: u32 = 0x40_0000;
const LOG_START: u32 = META + 0x2000;
const LOG_END: u32 = 0xC0_0000;
/// The `control` partition: the trusted key and name in one sector, the counter in the next, so
/// the frequent write cannot damage the rare one.
const IDENTITY: u32 = 0x9000;
const COUNTER: Ring = Ring {
    start: 0xA000,
    sectors: 1,
    slot: 8,
};
const BOOT_LOG: Ring = Ring {
    start: 0xB000,
    sectors: 2,
    slot: 4,
};
const SETTINGS: Ring = Ring {
    start: META + 0x1000,
    sectors: 1,
    slot: SETTINGS_LEN as u32,
};
const IDENTITY_LEN: usize = 8 + 33 + 32;
const MAGIC: u32 = 0x4F4C_4331;
const META_LEN: usize = 4 + 32 + 32;
const RECORD_HEADER: usize = 8;
const MAX_RECORD: usize = 64 * 1024;

const SETTINGS_LEN: usize = 256;
/// How far back to look when the newest settings were cut short.
const SETTINGS_KEPT: usize = 2;

/// Set with the `wifi`, `dial`, `view`, `lang` and `limit` commands. Empty means off or the
/// default. Each is one line, so settings saved before a line existed still load.
#[derive(Default)]
pub struct Settings {
    pub ssid: String,
    pub password: String,
    pub dial: String,
    pub view: String,
    pub language: String,
    /// What builds before `cap` saved. A number other than 0 still counts as a choice.
    pub limit: String,
    /// The chosen hash cap, 0 for full. Empty means the owner never chose.
    pub cap: String,
    /// Frames a second of the coin. Empty means the owner never chose.
    pub smooth: String,
    /// Radio power in dBm. Empty means the owner never chose.
    pub power: String,
    /// The panel's contrast. Empty means the owner never chose.
    pub bright: String,
    /// `off` keeps the board's own network down. Empty means up.
    pub own_network: String,
    /// Processor speed in MHz. Empty means the default.
    pub clock: String,
}

/// Who may command this board, and what it is called.
#[derive(Default)]
pub struct Identity {
    pub key: Option<PublicKey>,
    pub name: String,
}

pub struct Store {
    flash: FlashStorage<'static>,
    head: u32,
    genesis_matches: bool,
    /// This start's place in the boot log.
    boot_word: Option<(u32, u8)>,
}

/// `FlashStorage` as the wear rings see it.
struct Nor<'a>(&'a mut FlashStorage<'static>);

impl Flash for Nor<'_> {
    fn read(&mut self, at: u32, bytes: &mut [u8]) -> bool {
        self.0.read(at, bytes).is_ok()
    }

    fn program(&mut self, at: u32, bytes: &[u8]) -> bool {
        self.0.write_nor(at, bytes).is_ok()
    }

    fn erase(&mut self, sector: u32) -> bool {
        self.0
            .erase(sector, sector + olc_control::wear::SECTOR)
            .is_ok()
    }
}

fn is_ota(kind: AppPartitionSubType) -> bool {
    matches!(kind, AppPartitionSubType::Ota0 | AppPartitionSubType::Ota1)
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
            boot_word: None,
        }
    }

    /// Notes this start.
    ///
    /// Runs before there is a heap, so that a start that dies early still leaves its word.
    pub fn begin_boot(&mut self, reason: u8) {
        let mut flash = Nor(&mut self.flash);
        COUNTER.adopt(&mut flash);
        SETTINGS.adopt(&mut flash);
        let at = BOOT_LOG.next(&mut flash);
        self.boot_word = Some((at, reason));
        self.reach(Stage::Started);
        BOOT_LOG.written(&mut Nor(&mut self.flash), at);
    }

    pub fn reach(&mut self, stage: Stage) {
        if let Some((at, reason)) = self.boot_word {
            let bytes = bootlog::word(reason, stage).to_le_bytes();
            let _ = self.flash.write_nor(at, &bytes);
        }
    }

    /// The saved words of recent starts, oldest first.
    pub fn boot_words(&mut self, count: usize) -> Vec<u32> {
        let mut flash = Nor(&mut self.flash);
        let places = BOOT_LOG.newest_first(&mut flash, count);
        let mut words: Vec<u32> = places
            .iter()
            .map(|at| {
                let mut bytes = [0u8; 4];
                flash.read(*at, &mut bytes);
                word(&bytes)
            })
            .collect();
        words.reverse();
        words
    }

    pub fn settings(&mut self) -> Settings {
        let mut bytes = [0u8; SETTINGS_LEN];
        let mut flash = Nor(&mut self.flash);
        let found = SETTINGS
            .newest_first(&mut flash, SETTINGS_KEPT)
            .into_iter()
            .any(|at| flash.read(at, &mut bytes) && word(&bytes) == MAGIC);
        if !found {
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
            limit: lines.next().unwrap_or_default(),
            cap: lines.next().unwrap_or_default(),
            smooth: lines.next().unwrap_or_default(),
            power: lines.next().unwrap_or_default(),
            bright: lines.next().unwrap_or_default(),
            own_network: lines.next().unwrap_or_default(),
            clock: lines.next().unwrap_or_default(),
        }
    }

    pub fn save_settings(&mut self, settings: &Settings) {
        let text = alloc::format!(
            "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
            settings.ssid,
            settings.password,
            settings.dial,
            settings.view,
            settings.language,
            settings.limit,
            settings.cap,
            settings.smooth,
            settings.power,
            settings.bright,
            settings.own_network,
            settings.clock,
        );
        let mut bytes = [0u8; SETTINGS_LEN];
        if text.len() + RECORD_HEADER <= SETTINGS_LEN {
            bytes[..4].copy_from_slice(&MAGIC.to_le_bytes());
            bytes[4..8].copy_from_slice(&(text.len() as u32).to_le_bytes());
            bytes[RECORD_HEADER..][..text.len()].copy_from_slice(text.as_bytes());
            // The mark goes last, so a write cut short is passed over when read.
            let mut flash = Nor(&mut self.flash);
            let at = SETTINGS.next(&mut flash);
            flash.program(at + 4, &bytes[4..]);
            flash.program(at, &bytes[..4]);
        }
    }

    pub fn identity(&mut self) -> Identity {
        let mut bytes = [0u8; IDENTITY_LEN];
        let _ = self.flash.read(IDENTITY, &mut bytes);
        if word(&bytes) != MAGIC {
            return Identity::default();
        }
        let mut key: PublicKey = [0; 33];
        key.copy_from_slice(&bytes[8..41]);
        let size = usize::min(word(&bytes[4..]) as usize, IDENTITY_LEN - 41);
        Identity {
            key: bytes[8..41].iter().any(|byte| *byte != 0).then_some(key),
            name: core::str::from_utf8(&bytes[41..][..size])
                .unwrap_or("")
                .into(),
        }
    }

    pub fn save_identity(&mut self, identity: &Identity) {
        let mut bytes = [0u8; IDENTITY_LEN];
        let size = identity.name.len().min(IDENTITY_LEN - 41);
        bytes[..4].copy_from_slice(&MAGIC.to_le_bytes());
        bytes[4..8].copy_from_slice(&(size as u32).to_le_bytes());
        bytes[8..41].copy_from_slice(&identity.key.unwrap_or([0; 33]));
        bytes[41..][..size].copy_from_slice(&identity.name.as_bytes()[..size]);
        let _ = self.flash.write(IDENTITY, &bytes);
    }

    /// The last order counter accepted. Erased flash reads as none used.
    pub fn counter(&mut self) -> u64 {
        let mut bytes = [0u8; 8];
        let mut flash = Nor(&mut self.flash);
        match COUNTER.newest(&mut flash) {
            Some(at) if flash.read(at, &mut bytes) => u64::from_le_bytes(bytes),
            _ => 0,
        }
    }

    pub fn save_counter(&mut self, counter: u64) -> bool {
        let mut flash = Nor(&mut self.flash);
        let at = COUNTER.next(&mut flash);
        flash.program(at, &counter.to_le_bytes())
    }

    /// The app slot that is not running, as (start, size).
    pub fn spare_slot(&mut self) -> Option<(u32, u32)> {
        let mut buffer = [0u8; PARTITION_TABLE_MAX_LEN];
        let table = read_partition_table(&mut self.flash, &mut buffer).ok()?;
        let running = table.booted_partition().ok()??.offset();
        let spare = table.iter().find(|entry| {
            let app = matches!(entry.partition_type(), PartitionType::App(kind) if is_ota(kind));
            app && entry.offset() != running
        });
        spare.map(|entry| (entry.offset(), entry.len()))
    }

    /// 0 or 1, the app slot this build runs from.
    pub fn running_slot(&mut self) -> u8 {
        let mut buffer = [0u8; PARTITION_TABLE_MAX_LEN];
        let booted = read_partition_table(&mut self.flash, &mut buffer)
            .ok()
            .and_then(|table| table.booted_partition().ok().flatten());
        match booted.map(|entry| entry.partition_type()) {
            Some(PartitionType::App(AppPartitionSubType::Ota1)) => 1,
            _ => 0,
        }
    }

    /// Makes the spare slot the one to boot.
    pub fn boot_spare_slot(&mut self) -> bool {
        let target = match self.running_slot() {
            0 => AppPartitionSubType::Ota1,
            _ => AppPartitionSubType::Ota0,
        };
        let mut buffer = [0u8; PARTITION_TABLE_MAX_LEN];
        let Ok(mut updater) = OtaUpdater::new(&mut self.flash, &mut buffer) else {
            return false;
        };
        let Ok(mut data) = updater.ota_data() else {
            return false;
        };
        data.set_current_app_partition(target).is_ok()
            && data.set_current_ota_state(OtaImageState::New).is_ok()
    }

    /// Says this build started, and returns true on the first start after an update. A bootloader
    /// with rollback reads this; the one espflash installs does not, so an image that crashes at
    /// start stays selected.
    pub fn mark_running_build_good(&mut self) -> bool {
        let mut buffer = [0u8; PARTITION_TABLE_MAX_LEN];
        let Ok(mut updater) = OtaUpdater::new(&mut self.flash, &mut buffer) else {
            return false;
        };
        let is_new = updater
            .current_ota_state()
            .is_ok_and(|state| state == OtaImageState::New);
        let _ = updater.set_current_ota_state(OtaImageState::Valid);
        is_new
    }

    /// Bytes of block log still free.
    pub fn log_left(&self) -> u32 {
        LOG_END.saturating_sub(self.head)
    }

    pub fn write_page(&mut self, at: u32, page: &[u8]) -> bool {
        self.flash.write(at, page).is_ok()
    }

    pub fn read_flash(&mut self, at: u32, bytes: &mut [u8]) -> bool {
        self.flash.read(at, bytes).is_ok()
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
