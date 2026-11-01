//! A copy of the unspent set from a peer, for when nobody holds the first blocks any more. Like
//! Bitcoin Core's assumeutxo it trusts that peer: headers near the tip are checked, the history
//! below them cannot be. A person chooses this, the chain remembers it, and nothing does it alone.
use crate::chain::Chain;
use crate::ledger::{Found, UnspentSet};
use crate::log::NOWHERE;
use crate::tree::{Entries, Entry, Mark, MARK_EVERY};
use crate::types::BlockHeader;
use alloc::vec::Vec;
use olc_crypto::Hash;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotError {
    Unreadable,
    OtherCoin,
    /// It does not reach past this node's own chain.
    NotAhead,
    BadHeaders,
    /// It holds more coins than the chain can have made.
    TooManyCoins,
}

#[derive(Serialize, Deserialize)]
struct Snapshot {
    genesis: Hash,
    height: u64,
    total_work: u128,
    /// The headers a retarget needs, oldest first.
    headers: Vec<BlockHeader>,
    marks: Vec<(u64, Hash)>,
    unspent: UnspentSet,
}

impl Chain {
    /// This node's unspent set and recent headers, for a peer that asks.
    pub fn snapshot(&self) -> Vec<u8> {
        let mut headers = Vec::new();
        let mut cursor = Some(self.tip_entry());
        while let Some(entry) = cursor.filter(|_| (headers.len() as u64) < self.window()) {
            if entry.height == 0 {
                break;
            }
            headers.push(entry.header);
            cursor = self.parent_of(&entry);
        }
        headers.reverse();
        let snapshot = Snapshot {
            genesis: self.genesis,
            height: self.height(),
            total_work: self.total_work(),
            headers,
            marks: self
                .marks
                .iter()
                .filter(|mark| mark.height.is_multiple_of(MARK_EVERY))
                .map(|mark| (mark.height, mark.id))
                .collect(),
            unspent: self.unspent.clone(),
        };
        postcard::to_allocvec(&snapshot).unwrap_or_default()
    }

    /// Starts this chain from a peer's snapshot. Blocks held here stay unread and give way.
    pub fn adopt(&mut self, bytes: &[u8], peer: u32) -> Result<u64, SnapshotError> {
        let snapshot: Snapshot = postcard::from_bytes(bytes).map_err(|_| SnapshotError::Unreadable)?;
        if snapshot.genesis != self.genesis {
            return Err(SnapshotError::OtherCoin);
        }
        if snapshot.total_work <= self.total_work() {
            return Err(SnapshotError::NotAhead);
        }
        let entries = self.check_headers(&snapshot)?;
        let rules = &self.config.rules;
        let premine: u64 = self.config.premine.iter().map(|(_, value)| *value).sum();
        let most = rules.reward.saturating_mul(snapshot.height).saturating_add(premine);
        let coins = snapshot.unspent.values().map(|entry| entry.output.value);
        if coins.fold(0u64, u64::saturating_add) > most {
            return Err(SnapshotError::TooManyCoins);
        }
        self.tip = snapshot.headers.last().map_or(self.genesis, BlockHeader::id);
        self.entries = entries;
        self.marks = snapshot
            .marks
            .iter()
            .map(|(height, id)| Mark {
                height: *height,
                at: NOWHERE,
                id: *id,
            })
            .collect();
        self.unspent = snapshot.unspent;
        self.found = Found::default();
        self.assumed_from = Some(peer);
        let head = self.log.head();
        self.log.place(head, head);
        self.log.resume();
        self.save_now();
        Ok(snapshot.height)
    }

    fn check_headers(&self, snapshot: &Snapshot) -> Result<Entries, SnapshotError> {
        let consensus = &self.plugins.consensus;
        let count = snapshot.headers.len() as u64;
        if count == 0 || count > snapshot.height {
            return Err(SnapshotError::BadHeaders);
        }
        let mut entries = Entries::new();
        entries.insert(self.genesis, self.entries[&self.genesis]);
        let mut parent: Option<Hash> = None;
        let mut work_below = snapshot.total_work;
        let works = snapshot.headers.iter().map(|header| consensus.work(header));
        work_below = work_below.saturating_sub(works.sum());
        for (index, header) in snapshot.headers.iter().enumerate() {
            let links = parent.is_none_or(|id| id == header.previous);
            if !links || !consensus.check_seal(header) {
                return Err(SnapshotError::BadHeaders);
            }
            work_below += consensus.work(header);
            let id = header.id();
            let entry = Entry {
                header: *header,
                height: snapshot.height - count + 1 + index as u64,
                total_work: work_below,
                at: NOWHERE,
                parent_at: NOWHERE,
                validated: true,
            };
            entries.insert(id, entry);
            parent = Some(id);
        }
        Ok(entries)
    }
}
