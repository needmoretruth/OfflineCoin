//! Flash storage: the keys, the coin and the settings. The chain's own stores sit beside them, so
//! a board survives losing power.
use crate::flash::{self, Region};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use esp_bootloader_esp_idf::ota::OtaImageState;
use esp_bootloader_esp_idf::ota_updater::OtaUpdater;
use esp_bootloader_esp_idf::partitions::{
    read_partition_table, AppPartitionSubType, PartitionType, PARTITION_TABLE_MAX_LEN,
};
use olc_control::bootlog::{self, Stage};
use olc_control::wear::{Flash, Ring};
use olc_core::medium::Keep;
use olc_core::{Block, CoinConfig, Hash};
use olc_crypto::PublicKey;

/// The `chain` partition, past the end of the first app slot. See `partitions.csv`.
const META: u32 = 0x40_0000;
const SETTINGS: Ring = Ring {
    start: META + 0x1000,
    sectors: 1,
    slot: SETTINGS_LEN as u32,
};
/// Where builds before the ring kept every block. Read once, then it goes.
const OLD_LOG: u32 = META + 0x2000;
const OLD_LOG_END: u32 = KEYS;
const STATES: u32 = META + 0x1_0000;
const STATE_SIZE: u32 = 0x10_0000;
const LOG: u32 = STATES + 2 * STATE_SIZE;
const LOG_SIZE: u32 = KEYS - LOG;
/// Two sectors each, written in turn, so a power cut always leaves one whole.
const KEYS: u32 = 0xBF_0000;
const COIN_SLOTS: u32 = KEYS + 2 * SECTOR;
/// How the node runs, kept apart from `SETTINGS` so older records there still read.
const CHOICES: Ring = Ring {
    start: COIN_SLOTS + 2 * SECTOR,
    sectors: 1,
    slot: SETTINGS_LEN as u32,
};
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
const SECTOR: u32 = 0x1000;
const IDENTITY_LEN: usize = 8 + 33 + 32;
const MAGIC: u32 = 0x4F4C_4331;
const MAGIC_SLOT: u32 = 0x4F4C_4332;
const SLOT_HEADER: usize = 4 + 4 + 4 + 8;
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
    /// `on` brings up the board's own network. Empty means down.
    pub own_network: String,
    /// Processor speed in MHz. Empty means the default.
    pub clock: String,
    /// `on` starts Bluetooth. Empty means off.
    pub bluetooth: String,
    /// The order links are preferred in. Empty means the usual one.
    pub links: String,
    /// Where mined coins go. Empty means this board's own key.
    pub miner: String,
    /// Words to put in each mined block, as hex.
    pub note: String,
    /// Recent blocks to keep. Empty means all that fit.
    pub recent: String,
    /// "off" stops the wallet merging small coins.
    pub merge: String,
    /// "on" holds headers only, and asks full nodes for its coins.
    pub light: String,
}

/// Who may command this board, and what it is called.
#[derive(Default)]
pub struct Identity {
    pub key: Option<PublicKey>,
    pub name: String,
}

#[derive(Default)]
pub struct Store {
    /// This start's place in the boot log.
    boot_word: Option<(u32, u8)>,
}

/// The chip as the wear rings see it.
struct Nor;

impl Flash for Nor {
    fn read(&mut self, at: u32, bytes: &mut [u8]) -> bool {
        flash::read(at, bytes)
    }

    fn program(&mut self, at: u32, bytes: &[u8]) -> bool {
        flash::program(at, bytes)
    }

    fn erase(&mut self, sector: u32) -> bool {
        flash::erase(sector)
    }
}

/// The newest whole text in a ring of settings.
fn read_text(ring: &Ring) -> String {
    let mut bytes = [0u8; SETTINGS_LEN];
    let found = ring
        .newest_first(&mut Nor, SETTINGS_KEPT)
        .into_iter()
        .any(|at| flash::read(at, &mut bytes) && word(&bytes) == MAGIC);
    if !found {
        return String::new();
    }
    let size = usize::min(word(&bytes[4..]) as usize, SETTINGS_LEN - RECORD_HEADER);
    core::str::from_utf8(&bytes[RECORD_HEADER..][..size])
        .unwrap_or("")
        .into()
}

fn write_text(ring: &Ring, text: &str) {
    let mut bytes = [0u8; SETTINGS_LEN];
    if text.len() + RECORD_HEADER <= SETTINGS_LEN {
        bytes[..4].copy_from_slice(&MAGIC.to_le_bytes());
        bytes[4..8].copy_from_slice(&(text.len() as u32).to_le_bytes());
        bytes[RECORD_HEADER..][..text.len()].copy_from_slice(text.as_bytes());
        // The mark goes last, so a write cut short is passed over when read.
        let at = ring.next(&mut Nor);
        flash::program(at + 4, &bytes[4..]);
        flash::program(at, &bytes[..4]);
    }
}

fn is_ota(kind: AppPartitionSubType) -> bool {
    matches!(kind, AppPartitionSubType::Ota0 | AppPartitionSubType::Ota1)
}

fn word(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

impl Store {
    /// Notes this start. Writes without erasing, and a start that died before its next stage
    /// shares the word of the last one like it, so a restart loop wears nothing.
    ///
    /// Runs before there is a heap, so that a start that dies early still leaves its word.
    pub fn begin_boot(&mut self, reason: u8) {
        COUNTER.adopt(&mut Nor);
        SETTINGS.adopt(&mut Nor);
        CHOICES.adopt(&mut Nor);
        let same = BOOT_LOG.newest(&mut Nor).filter(|at| {
            let mut bytes = [0u8; 4];
            flash::read(*at, &mut bytes) && word(&bytes) == bootlog::word(reason, Stage::Started)
        });
        if let Some(at) = same {
            self.boot_word = Some((at, reason));
            return;
        }
        let at = BOOT_LOG.next(&mut Nor);
        self.boot_word = Some((at, reason));
        self.reach(Stage::Started);
        BOOT_LOG.written(&mut Nor, at);
    }

    pub fn reach(&mut self, stage: Stage) {
        if let Some((at, reason)) = self.boot_word {
            let bytes = bootlog::word(reason, stage).to_le_bytes();
            flash::program(at, &bytes);
        }
    }

    /// The saved words of recent starts, oldest first.
    pub fn boot_words(&mut self, count: usize) -> Vec<u32> {
        let places = BOOT_LOG.newest_first(&mut Nor, count);
        let mut words: Vec<u32> = places
            .iter()
            .map(|at| {
                let mut bytes = [0u8; 4];
                flash::read(*at, &mut bytes);
                word(&bytes)
            })
            .collect();
        words.reverse();
        words
    }

    pub fn settings(&mut self) -> Settings {
        let text = read_text(&SETTINGS);
        let mut lines = text.split('\n').map(String::from);
        let choices = read_text(&CHOICES);
        let mut choice = choices.split('\n').map(String::from);
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
            bluetooth: lines.next().unwrap_or_default(),
            links: lines.next().unwrap_or_default(),
            miner: choice.next().unwrap_or_default(),
            note: choice.next().unwrap_or_default(),
            recent: choice.next().unwrap_or_default(),
            merge: choice.next().unwrap_or_default(),
            light: choice.next().unwrap_or_default(),
        }
    }

    pub fn save_settings(&mut self, settings: &Settings) {
        let text = alloc::format!(
            "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
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
            settings.bluetooth,
            settings.links,
        );
        write_text(&SETTINGS, &text);
        let choices = alloc::format!(
            "{}\n{}\n{}\n{}\n{}",
            settings.miner,
            settings.note,
            settings.recent,
            settings.merge,
            settings.light,
        );
        write_text(&CHOICES, &choices);
    }

    pub fn identity(&mut self) -> Identity {
        let mut bytes = [0u8; IDENTITY_LEN];
        flash::read(IDENTITY, &mut bytes);
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
        flash::replace(IDENTITY, &bytes);
    }

    /// The last order counter accepted. Erased flash reads as none used.
    pub fn counter(&mut self) -> u64 {
        let mut bytes = [0u8; 8];
        match COUNTER.newest(&mut Nor) {
            Some(at) if flash::read(at, &mut bytes) => u64::from_le_bytes(bytes),
            _ => 0,
        }
    }

    pub fn save_counter(&mut self, counter: u64) -> bool {
        let at = COUNTER.next(&mut Nor);
        flash::program(at, &counter.to_le_bytes())
    }

    /// The app slot that is not running, as (start, size).
    pub fn spare_slot(&mut self) -> Option<(u32, u32)> {
        let mut buffer = [0u8; PARTITION_TABLE_MAX_LEN];
        flash::with(|flash| {
            let table = read_partition_table(flash, &mut buffer).ok()?;
            let running = table.booted_partition().ok()??.offset();
            let spare = table.iter().find(|entry| {
                let app =
                    matches!(entry.partition_type(), PartitionType::App(kind) if is_ota(kind));
                app && entry.offset() != running
            });
            spare.map(|entry| (entry.offset(), entry.len()))
        })?
    }

    /// Where the image of the running build starts.
    pub fn running_start(&mut self) -> Option<u32> {
        let mut buffer = [0u8; PARTITION_TABLE_MAX_LEN];
        let table = read_partition_table(&mut self.flash, &mut buffer).ok()?;
        let booted = table.booted_partition().ok()??;
        Some(booted.offset())
    }

    /// 0 or 1, the app slot this build runs from.
    pub fn running_slot(&mut self) -> u8 {
        let mut buffer = [0u8; PARTITION_TABLE_MAX_LEN];
        let booted = flash::with(|flash| {
            read_partition_table(flash, &mut buffer)
                .ok()
                .and_then(|table| table.booted_partition().ok().flatten())
                .map(|entry| entry.partition_type())
        });
        match booted.flatten() {
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
        flash::with(|flash| {
            let Ok(mut updater) = OtaUpdater::new(flash, &mut buffer) else {
                return false;
            };
            let Ok(mut data) = updater.ota_data() else {
                return false;
            };
            data.set_current_app_partition(target).is_ok()
                && data.set_current_ota_state(OtaImageState::New).is_ok()
        })
        .unwrap_or(false)
    }

    /// Says this build started, and returns true on the first start after an update. A bootloader
    /// with rollback reads this; the one espflash installs does not, so an image that crashes at
    /// start stays selected.
    pub fn mark_running_build_good(&mut self) -> bool {
        let mut buffer = [0u8; PARTITION_TABLE_MAX_LEN];
        flash::with(|flash| {
            let Ok(mut updater) = OtaUpdater::new(flash, &mut buffer) else {
                return false;
            };
            let is_new = updater
                .current_ota_state()
                .is_ok_and(|state| state == OtaImageState::New);
            let _ = updater.set_current_ota_state(OtaImageState::Valid);
            is_new
        })
        .unwrap_or(false)
    }

    pub fn write_page(&mut self, at: u32, page: &[u8]) -> bool {
        flash::replace(at, page)
    }

    pub fn read_flash(&mut self, at: u32, bytes: &mut [u8]) -> bool {
        flash::read(at, bytes)
    }

    /// The chain's stores on this board.
    pub fn keep(&self, recent: Option<u64>) -> Keep {
        let log = Region {
            start: LOG,
            size: LOG_SIZE,
        };
        let state = |index| Region {
            start: STATES + index * STATE_SIZE,
            size: STATE_SIZE,
        };
        flash::keep(log, [state(0), state(1)], recent)
    }

    /// The saved keys and the one that receives. A key outlives a change of coin, so it has its
    /// own place, and the first start on this layout takes the key builds before it saved.
    pub fn keys(&mut self, create: impl FnOnce() -> [u8; 32]) -> (Vec<[u8; 32]>, usize) {
        if let Some(saved) = read_slot(KEYS).and_then(|bytes| postcard::from_bytes(&bytes).ok()) {
            return saved;
        }
        let saved = (vec![self.old_secret().unwrap_or_else(create)], 0);
        self.save_keys(&saved.0, saved.1);
        saved
    }

    pub fn save_keys(&mut self, secrets: &[[u8; 32]], receiving: usize) -> bool {
        postcard::to_allocvec(&(secrets, receiving)).is_ok_and(|bytes| write_slot(KEYS, &bytes))
    }

    /// The coin this board runs, when it was told one.
    pub fn coin(&mut self) -> Option<CoinConfig> {
        read_slot(COIN_SLOTS).and_then(|bytes| CoinConfig::from_bytes(&bytes))
    }

    pub fn save_coin(&mut self, coin: &CoinConfig) -> bool {
        write_slot(COIN_SLOTS, &coin.to_bytes())
    }

    fn old_meta(&mut self) -> Option<[u8; META_LEN]> {
        let mut meta = [0u8; META_LEN];
        flash::read(META, &mut meta);
        (word(&meta) == MAGIC).then_some(meta)
    }

    fn old_secret(&mut self) -> Option<[u8; 32]> {
        let meta = self.old_meta()?;
        let mut secret = [0u8; 32];
        secret.copy_from_slice(&meta[36..]);
        Some(secret)
    }

    /// Every block a build before the ring saved for this coin, in the order they came.
    pub fn old_blocks(&mut self, genesis: &Hash) -> Vec<Block> {
        let mut blocks = Vec::new();
        if self.old_meta().is_none_or(|meta| meta[4..36] != genesis[..]) {
            return blocks;
        }
        let mut at = OLD_LOG;
        while at + (RECORD_HEADER as u32) < OLD_LOG_END {
            let mut header = [0u8; RECORD_HEADER];
            flash::read(at, &mut header);
            let size = word(&header[4..]) as usize;
            if word(&header) != MAGIC || size > MAX_RECORD {
                break;
            }
            let mut bytes = vec![0u8; size];
            flash::read(at + RECORD_HEADER as u32, &mut bytes);
            match postcard::from_bytes(&bytes) {
                Ok(block) => blocks.push(block),
                Err(_) => break,
            }
            at += (RECORD_HEADER + size.next_multiple_of(4)) as u32;
        }
        blocks
    }

    /// Ends the old layout. Until this runs, a power cut just reads it again.
    pub fn forget_old_blocks(&mut self) {
        if self.old_meta().is_some() {
            flash::erase(META);
        }
    }
}

fn slot_at(place: u32, index: u32) -> u32 {
    place + index * SECTOR
}

/// A slot's sequence number and payload, when it is whole.
fn read_one(at: u32) -> Option<(u32, Vec<u8>)> {
    let mut header = [0u8; SLOT_HEADER];
    flash::read(at, &mut header);
    let size = word(&header[8..]) as usize;
    if word(&header) != MAGIC_SLOT || size > SECTOR as usize - SLOT_HEADER {
        return None;
    }
    let mut payload = vec![0u8; size];
    flash::read(at + SLOT_HEADER as u32, &mut payload);
    (olc_crypto::hash(&payload)[..8] == header[12..]).then(|| (word(&header[4..]), payload))
}

fn read_slot(place: u32) -> Option<Vec<u8>> {
    let first = read_one(slot_at(place, 0));
    let second = read_one(slot_at(place, 1));
    match (first, second) {
        (Some(a), Some(b)) => Some(if b.0 > a.0 { b.1 } else { a.1 }),
        (one, other) => one.or(other).map(|slot| slot.1),
    }
}

/// Writes over the older of the two, so the newer stays whole until this one is.
fn write_slot(place: u32, payload: &[u8]) -> bool {
    if payload.len() > SECTOR as usize - SLOT_HEADER {
        return false;
    }
    let first = read_one(slot_at(place, 0)).map(|slot| slot.0);
    let second = read_one(slot_at(place, 1)).map(|slot| slot.0);
    let (index, sequence) = match (first, second) {
        (Some(a), Some(b)) if a >= b => (1, a + 1),
        (Some(_), Some(b)) => (0, b + 1),
        (Some(a), None) => (1, a + 1),
        (None, Some(b)) => (0, b + 1),
        (None, None) => (0, 1),
    };
    let at = slot_at(place, index);
    let mut record = Vec::with_capacity(SLOT_HEADER + payload.len() + 3);
    record.extend_from_slice(&MAGIC_SLOT.to_le_bytes());
    record.extend_from_slice(&sequence.to_le_bytes());
    record.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    record.extend_from_slice(&olc_crypto::hash(payload)[..8]);
    record.extend_from_slice(payload);
    record.resize(record.len().next_multiple_of(4), 0xFF);
    flash::erase(at) && flash::program(at, &record)
}
