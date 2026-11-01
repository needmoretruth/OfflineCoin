//! The block log: records one after another on a [`Medium`], oldest bytes giving way to new ones.
//!
//! A record is whole or it does not count: its checksum covers the genesis, its position and its
//! bytes, so a torn write, an old record from the last time around and another coin's record all
//! read as nothing. After a torn write the next record starts on a fresh sector.
use crate::ledger::Unspent;
use crate::medium::{Medium, SECTOR};
use crate::types::{Block, BlockHeader, Tx};
use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use olc_crypto::{Hash, StreamHash};

/// Where a block that is in no record points.
pub(crate) const NOWHERE: u64 = u64::MAX;
const MAGIC: u32 = 0x4F4C_4752;
/// Magic, kind and length, position, checksum.
const HEADER: usize = 24;
/// Parent's position, height, total work and block header.
const META: usize = 8 + 8 + 16 + BlockHeader::ENCODED_LEN;
/// Far more than a block and its undo data.
const MAX_RECORD: usize = 256 * 1024;
const WORD: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A block joined the best chain. Holds what undoing it takes.
    Tip = 1,
    /// A block is stored on a branch with less work.
    Side = 2,
    /// The best chain went back to a block.
    Rewind = 3,
}

impl Kind {
    fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            1 => Some(Self::Tip),
            2 => Some(Self::Side),
            3 => Some(Self::Rewind),
            _ => None,
        }
    }
}

/// The fixed part of a block's record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Meta {
    pub parent_at: u64,
    pub height: u64,
    pub total_work: u128,
    pub header: BlockHeader,
}

impl Meta {
    fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.parent_at.to_le_bytes());
        out.extend_from_slice(&self.height.to_le_bytes());
        out.extend_from_slice(&self.total_work.to_le_bytes());
        out.extend_from_slice(&self.header.encode());
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        Some(Self {
            parent_at: u64::from_le_bytes(bytes.get(0..8)?.try_into().ok()?),
            height: u64::from_le_bytes(bytes.get(8..16)?.try_into().ok()?),
            total_work: u128::from_le_bytes(bytes.get(16..32)?.try_into().ok()?),
            header: BlockHeader::decode(bytes.get(32..META)?)?,
        })
    }
}

fn sector_floor(at: u64) -> u64 {
    at - at % SECTOR
}

/// Encodes a block's record: its fixed part, its transactions and, on a tip, its undo data.
pub(crate) fn block_record(meta: &Meta, txs: &[Tx], undo: Option<&[Unspent]>) -> Vec<u8> {
    let mut out = Vec::with_capacity(META + 256);
    meta.encode(&mut out);
    out.extend(postcard::to_allocvec(txs).unwrap_or_default());
    if let Some(undo) = undo {
        out.extend(postcard::to_allocvec(undo).unwrap_or_default());
    }
    out
}

/// A block's record read back.
pub(crate) struct Read {
    pub meta: Meta,
    pub block: Block,
    pub undo: Vec<Unspent>,
}

pub(crate) fn read_block_record(bytes: &[u8]) -> Option<Read> {
    let meta = Meta::decode(bytes)?;
    let (txs, rest): (Vec<Tx>, _) = postcard::take_from_bytes(&bytes[META..]).ok()?;
    let undo = postcard::from_bytes(rest).unwrap_or_default();
    let block = Block {
        header: meta.header,
        txs,
    };
    Some(Read { meta, block, undo })
}

/// Where a log's records turned out to be.
pub(crate) struct Survey {
    pub lowest: u64,
    pub end: u64,
}

pub struct Log {
    medium: Box<dyn Medium>,
    /// The genesis id, which makes another coin's records unreadable here.
    salt: Hash,
    head: u64,
    tail: u64,
}

impl Log {
    pub(crate) fn new(medium: Box<dyn Medium>, salt: Hash) -> Self {
        Self {
            medium,
            salt,
            head: 0,
            tail: 0,
        }
    }

    pub fn head(&self) -> u64 {
        self.head
    }

    pub fn tail(&self) -> u64 {
        self.tail
    }

    /// Bytes this log holds now.
    pub fn held(&self) -> u64 {
        self.head - self.tail
    }

    pub fn capacity(&self) -> u64 {
        self.medium.capacity()
    }

    fn sum(&self, at: u64, kind: u8, payload: &[u8]) -> [u8; 8] {
        let mut hash = StreamHash::default();
        hash.update(&self.salt);
        hash.update(&at.to_le_bytes());
        hash.update(&[kind]);
        hash.update(payload);
        let digest = hash.finish();
        let mut sum = [0; 8];
        sum.copy_from_slice(&digest[..8]);
        sum
    }

    /// Adds a record and returns where it went. `None` leaves the record out and the log whole.
    pub(crate) fn append(&mut self, kind: Kind, payload: &[u8]) -> Option<u64> {
        let size = (HEADER + payload.len()).next_multiple_of(WORD);
        let capacity = self.medium.capacity();
        if payload.len() > MAX_RECORD || size as u64 + SECTOR > capacity {
            return None;
        }
        let at = self.head;
        let end = at + size as u64;
        let mut sector = at.next_multiple_of(SECTOR);
        while sector < end {
            self.tail = self.tail.max((sector + SECTOR).saturating_sub(capacity));
            if !self.medium.erase(sector) {
                return None;
            }
            sector += SECTOR;
        }
        let mut record = vec![0; size];
        record[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        record[4..8].copy_from_slice(&(((kind as u32) << 24) | payload.len() as u32).to_le_bytes());
        record[8..16].copy_from_slice(&at.to_le_bytes());
        record[16..24].copy_from_slice(&self.sum(at, kind as u8, payload));
        record[HEADER..HEADER + payload.len()].copy_from_slice(payload);
        if !self.medium.write(at, &record) {
            return None;
        }
        self.medium.sync();
        self.head = end;
        Some(at)
    }

    /// Kind and length of the record at `at`, if a record's start is there.
    fn start(&self, at: u64) -> Option<(Kind, usize)> {
        if at < self.tail || at == NOWHERE {
            return None;
        }
        let mut header = [0u8; HEADER];
        if !self.medium.read(at, &mut header) {
            return None;
        }
        let magic = u32::from_le_bytes(header[0..4].try_into().ok()?);
        let word = u32::from_le_bytes(header[4..8].try_into().ok()?);
        let place = u64::from_le_bytes(header[8..16].try_into().ok()?);
        let length = (word & 0x00FF_FFFF) as usize;
        if magic != MAGIC || place != at || length > MAX_RECORD {
            return None;
        }
        Some((Kind::from_byte((word >> 24) as u8)?, length))
    }

    /// The whole record at `at`, checked.
    pub(crate) fn record(&self, at: u64) -> Option<(Kind, Vec<u8>)> {
        let (kind, length) = self.start(at)?;
        let mut stored = [0u8; 8];
        let mut payload = vec![0; length];
        let read = self.medium.read(at + 16, &mut stored)
            && self.medium.read(at + HEADER as u64, &mut payload);
        (read && stored == self.sum(at, kind as u8, &payload)).then_some((kind, payload))
    }

    /// The fixed part of a block's record, without reading the block.
    pub(crate) fn meta(&self, at: u64) -> Option<Meta> {
        let (kind, length) = self.start(at)?;
        if kind == Kind::Rewind || length < META {
            return None;
        }
        let mut bytes = [0u8; META];
        self.medium
            .read(at + HEADER as u64, &mut bytes)
            .then(|| Meta::decode(&bytes))?
    }

    /// The record at `at` and where the next one starts, stepping over a torn record's sector.
    pub(crate) fn next(&self, at: u64) -> Option<(u64, Kind, Vec<u8>)> {
        let found = self.record(at).map(|(kind, payload)| (at, kind, payload));
        found.or_else(|| {
            let after = sector_floor(at) + SECTOR;
            let (kind, payload) = self.record(after)?;
            Some((after, kind, payload))
        })
    }

    /// Where the record after one of `length` payload bytes at `at` starts.
    pub(crate) fn after(at: u64, length: usize) -> u64 {
        at + (HEADER + length).next_multiple_of(WORD) as u64
    }

    /// Finds the records by reading every sector. Only for when no saved state says where they
    /// start.
    pub(crate) fn survey(&self) -> Option<Survey> {
        let capacity = self.medium.capacity();
        let unbounded = capacity == u64::MAX;
        let mut found: Option<Survey> = None;
        let mut sector = self.medium.lowest();
        let mut quiet = 0;
        // A record's start can reach past the end of a sector.
        let mut bytes = vec![0u8; SECTOR as usize + HEADER];
        while (sector < capacity) && !(unbounded && quiet >= 8) {
            let read = self.medium.read(sector, &mut bytes);
            let blank = !read || bytes[..SECTOR as usize].iter().all(|byte| *byte == 0xFF);
            quiet = if blank { quiet + 1 } else { 0 };
            for offset in (0..SECTOR as usize).step_by(WORD) {
                if blank || bytes.get(offset..offset + WORD) != Some(&MAGIC.to_le_bytes()[..]) {
                    continue;
                }
                let field = bytes.get(offset + 8..offset + 16);
                let Some(place) = field.and_then(|f| f.try_into().ok()).map(u64::from_le_bytes)
                else {
                    continue;
                };
                let physical = sector + offset as u64;
                if place % capacity != physical % capacity {
                    continue;
                }
                let Some((_, payload)) = self.record(place) else {
                    continue;
                };
                let end = Self::after(place, payload.len());
                let survey = found.get_or_insert(Survey { lowest: place, end });
                survey.lowest = survey.lowest.min(place);
                survey.end = survey.end.max(end);
            }
            sector += SECTOR;
        }
        found
    }

    /// Stands at `head` without looking at what is there.
    pub(crate) fn place(&mut self, head: u64, tail: u64) {
        self.head = head;
        self.tail = tail;
    }

    /// Takes up writing where [`Log::place`] stood. A cut write leaves dirt ahead of it that only
    /// an erase could clean, so the log then moves on to the next sector.
    pub(crate) fn resume(&mut self) {
        let head = self.head;
        if !head.is_multiple_of(SECTOR) {
            let mut rest = vec![0u8; (SECTOR - head % SECTOR) as usize];
            let clean = self.medium.read(head, &mut rest) && rest.iter().all(|byte| *byte == 0xFF);
            if !clean {
                self.head = sector_floor(head) + SECTOR;
            }
        }
        let capacity = self.medium.capacity();
        self.tail = self.tail.max((sector_floor(self.head) + SECTOR).saturating_sub(capacity));
    }

    /// Lets go of everything before `below`.
    pub(crate) fn forget(&mut self, below: u64) {
        self.tail = self.tail.max(below);
        self.medium.forget(self.tail);
    }

}
