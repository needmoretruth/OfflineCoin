//! A saved copy of what memory holds, so a boot reads a few records instead of every block.
//!
//! Two places take turns. A place first loses its header, then gets its bytes one sector at a
//! time, so a node never stalls, and last gets a header whose hash covers them. Whatever a power
//! cut interrupts, the other place stays whole.
use crate::chain::{Chain, ChainError};
use crate::coin::CoinConfig;
use crate::ledger::{apply_block, Found, UnspentSet};
use crate::log::{read_block_record, Kind, Log, Read, NOWHERE};
use crate::medium::{Keep, Medium, SECTOR};
use crate::orphans::Orphans;
use crate::plugin::Plugins;
use crate::tree::{Entries, Entry, Mark};
use alloc::vec;
use alloc::vec::Vec;
use olc_crypto::Hash;
use serde::{Deserialize, Serialize};

const MAGIC: u32 = 0x4F4C_5354;
const HEADER: usize = 4 + 8 + 8 + 32;
/// A boot reads at most about this many blocks.
const STATE_EVERY: u64 = 256;

/// What a chain found when it opened its store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opened {
    /// Nothing was saved yet.
    New,
    /// The saved state was whole. `read` records came after it.
    Resumed { read: u64 },
    /// The saved state was damaged, and the blocks held here gave the chain back.
    Rebuilt { read: u64 },
    /// The saved state was damaged and old blocks had gone, so the chain starts over from peers.
    Lost,
}

#[derive(Serialize, Deserialize)]
struct Saved {
    genesis: Hash,
    head: u64,
    tail: u64,
    tip: Hash,
    entries: Entries,
    marks: Vec<Mark>,
    unspent: UnspentSet,
    found: Found,
    assumed_from: Option<u32>,
}

/// The same fields in the same order, borrowed, so saving copies nothing.
#[derive(Serialize)]
struct SavedNow<'a> {
    genesis: &'a Hash,
    head: u64,
    tail: u64,
    tip: &'a Hash,
    entries: &'a Entries,
    marks: &'a Vec<Mark>,
    unspent: &'a UnspentSet,
    found: &'a Found,
    assumed_from: &'a Option<u32>,
}

/// A state on its way to its place.
pub(crate) struct Saving {
    place: usize,
    seq: u64,
    head: u64,
    bytes: Vec<u8>,
    sum: Hash,
    sent: usize,
}

fn holds_header(place: &dyn Medium) -> bool {
    let mut magic = [0u8; 4];
    place.read(0, &mut magic) && u32::from_le_bytes(magic) == MAGIC
}

fn read_place(place: &dyn Medium, genesis: &Hash) -> Option<(u64, Saved)> {
    let mut header = [0u8; HEADER];
    if !place.read(0, &mut header) {
        return None;
    }
    let magic = u32::from_le_bytes(header[0..4].try_into().ok()?);
    let seq = u64::from_le_bytes(header[4..12].try_into().ok()?);
    let length = u64::from_le_bytes(header[12..20].try_into().ok()?);
    if magic != MAGIC || length.saturating_add(SECTOR) > place.capacity() {
        return None;
    }
    let mut bytes = vec![0u8; length as usize];
    if !place.read(SECTOR, &mut bytes) || olc_crypto::hash(&bytes)[..] != header[20..52] {
        return None;
    }
    let saved: Saved = postcard::from_bytes(&bytes).ok()?;
    (saved.genesis == *genesis).then_some((seq, saved))
}

impl Chain {
    /// Opens the store and comes back to where the chain was.
    pub fn open(config: CoinConfig, plugins: Plugins, keep: Keep) -> Self {
        let genesis_block = config.genesis();
        let genesis = genesis_block.id();
        let mut unspent = UnspentSet::new();
        apply_block(&genesis_block, 0, &mut unspent);
        let entry = Entry {
            header: genesis_block.header,
            height: 0,
            total_work: 0,
            at: NOWHERE,
            parent_at: NOWHERE,
            validated: true,
        };
        let mut entries = Entries::new();
        entries.insert(genesis, entry);
        let Keep {
            log,
            states,
            recent,
        } = keep;
        let mut chain = Self {
            config,
            plugins,
            genesis,
            genesis_block,
            entries,
            marks: Vec::new(),
            orphans: Orphans::default(),
            tip: genesis,
            unspent,
            found: Found::default(),
            log: Log::new(log, genesis),
            places: states,
            recent,
            assumed_from: None,
            opened: Opened::New,
            saved_head: 0,
            saved_seq: 0,
            saved_height: 0,
            saving: None,
            state_too_big: false,
        };
        chain.come_back();
        chain
    }

    fn come_back(&mut self) {
        let places = [
            read_place(self.places[0].as_ref(), &self.genesis),
            read_place(self.places[1].as_ref(), &self.genesis),
        ];
        let damaged = self.places.iter().any(|place| holds_header(place.as_ref()));
        let [first, second] = places;
        let newest = match (first, second) {
            (Some(a), Some(b)) => Some(if a.0 >= b.0 { a } else { b }),
            (a, b) => a.or(b),
        };
        if let Some((seq, saved)) = newest {
            self.saved_seq = seq;
            self.saved_head = saved.head;
            self.saved_height = saved.entries.get(&saved.tip).map_or(0, |entry| entry.height);
            self.log.place(saved.head, saved.tail);
            self.entries = saved.entries;
            self.marks = saved.marks;
            self.unspent = saved.unspent;
            self.found = saved.found;
            self.tip = saved.tip;
            self.assumed_from = saved.assumed_from;
            let read = self.read_log(saved.head);
            self.opened = Opened::Resumed { read };
            self.finish_coming_back();
            return;
        }
        let Some(found) = self.log.survey() else {
            self.opened = if damaged { Opened::Lost } else { Opened::New };
            return;
        };
        let first_block = self.log.record(found.lowest).filter(|(kind, _)| *kind != Kind::Rewind);
        let joins = first_block
            .and_then(|(_, bytes)| read_block_record(&bytes))
            .is_some_and(|read| read.block.header.previous == self.genesis);
        if joins {
            self.log.place(found.lowest, found.lowest);
            let read = self.read_log(found.lowest);
            self.opened = match damaged {
                true => Opened::Rebuilt { read },
                false => Opened::Resumed { read },
            };
        } else {
            self.opened = Opened::Lost;
        }
        if self.log.head() < found.end {
            let tail = if joins { found.lowest } else { found.end };
            self.log.place(found.end, tail);
        }
        self.finish_coming_back();
        // The next boot should not have to work this out again.
        if damaged || self.opened == Opened::Lost {
            self.save_now();
        }
    }

    /// Follows the log from `start` and returns how many records that took.
    fn read_log(&mut self, start: u64) -> u64 {
        let mut at = start;
        let mut read_count = 0;
        while let Some((place, kind, payload)) = self.log.next(at) {
            let done = match kind {
                Kind::Rewind => self.read_rewind(&payload),
                _ => self.read_block(kind, place, &payload),
            };
            if !done {
                break;
            }
            read_count += 1;
            at = Log::after(place, payload.len());
            if read_count % 64 == 0 {
                self.settle();
            }
        }
        self.log.place(at, self.log.tail());
        read_count
    }

    fn read_block(&mut self, kind: Kind, at: u64, payload: &[u8]) -> bool {
        let Some(read) = read_block_record(payload) else {
            return false;
        };
        let Read { meta, block, .. } = read;
        let entry = Entry {
            header: block.header,
            height: meta.height,
            total_work: meta.total_work,
            at,
            parent_at: meta.parent_at,
            validated: kind == Kind::Tip,
        };
        match kind {
            Kind::Tip if block.header.previous == self.tip => self.step_forward(entry, &block),
            Kind::Tip => return false,
            _ => {
                self.entries.insert(block.id(), entry);
            }
        }
        true
    }

    fn read_rewind(&mut self, payload: &[u8]) -> bool {
        while self.tip.as_slice() != payload {
            let tip = self.tip_entry();
            let Some(read) = self.tip_read(&tip) else {
                return false;
            };
            self.step_back(&read);
            if !self.entries.contains_key(&self.tip) {
                let Some(parent) = self.parent_of(&tip) else {
                    return false;
                };
                self.entries.insert(self.tip, parent);
            }
        }
        true
    }

    /// A cut in the middle of a reorganisation leaves a heavier branch behind. Finish the move.
    fn finish_coming_back(&mut self) {
        self.log.resume();
        loop {
            let heaviest = self.entries.iter().max_by_key(|(_, entry)| entry.total_work);
            let Some((best, entry)) = heaviest.map(|(id, entry)| (*id, *entry)) else {
                break;
            };
            if entry.total_work <= self.total_work() {
                break;
            }
            match self.move_to(best) {
                Err(ChainError::ReorgTooDeep | ChainError::StoreFailed) | Ok(_) => break,
                Err(_) => {}
            }
        }
        self.settle();
    }

    /// One step of saving the state. Call it often: it writes `sectors` sectors at the most.
    pub fn keep_up(&mut self, sectors: usize) {
        if self.saving.is_none() && self.state_is_due() {
            self.begin_saving();
        }
        for _ in 0..sectors {
            let Some(mut saving) = self.saving.take() else {
                return;
            };
            if self.save_step(&mut saving) {
                self.saving = Some(saving);
            }
        }
    }

    /// Saves the state now, for a node about to stop.
    pub fn save_now(&mut self) {
        self.saving = None;
        self.begin_saving();
        self.keep_up(usize::MAX);
    }

    fn state_is_due(&self) -> bool {
        let moved = self.log.head() - self.saved_head;
        let blocks = self.height().abs_diff(self.saved_height);
        moved > 0 && (blocks >= STATE_EVERY || moved >= self.log.capacity() / 4)
    }

    fn begin_saving(&mut self) {
        let head = self.log.head();
        let saved = SavedNow {
            genesis: &self.genesis,
            head,
            tail: self.log.tail(),
            tip: &self.tip,
            entries: &self.entries,
            marks: &self.marks,
            unspent: &self.unspent,
            found: &self.found,
            assumed_from: &self.assumed_from,
        };
        let Ok(bytes) = postcard::to_allocvec(&saved) else {
            return;
        };
        let place = (self.saved_seq % 2) as usize;
        self.state_too_big = bytes.len() as u64 + SECTOR > self.places[place].capacity();
        self.saved_height = self.height();
        if self.state_too_big {
            return;
        }
        self.saving = Some(Saving {
            place,
            seq: self.saved_seq + 1,
            head,
            sum: olc_crypto::hash(&bytes),
            bytes,
            sent: 0,
        });
    }

    /// Writes one sector. True while more remains.
    fn save_step(&mut self, saving: &mut Saving) -> bool {
        let place = self.places[saving.place].as_mut();
        if saving.sent == 0 && !place.erase(0) {
            return false;
        }
        if saving.sent < saving.bytes.len() {
            let end = saving.bytes.len().min(saving.sent + SECTOR as usize);
            let mut part = saving.bytes[saving.sent..end].to_vec();
            part.resize(part.len().next_multiple_of(4), 0xFF);
            let at = SECTOR + saving.sent as u64;
            if !place.erase(at) || !place.write(at, &part) {
                return false;
            }
            saving.sent = end;
            return true;
        }
        let mut header = Vec::with_capacity(HEADER);
        header.extend_from_slice(&MAGIC.to_le_bytes());
        header.extend_from_slice(&saving.seq.to_le_bytes());
        header.extend_from_slice(&(saving.bytes.len() as u64).to_le_bytes());
        header.extend_from_slice(&saving.sum);
        place.sync();
        if place.write(0, &header) {
            place.sync();
            self.saved_seq = saving.seq;
            self.saved_head = saving.head;
        }
        false
    }
}
