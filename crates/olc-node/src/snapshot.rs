//! When nobody holds the first blocks any more: the node says so, and its owner may take a peer's
//! unspent set instead. Nothing here starts on its own.
use crate::engine::{Engine, PeerId};
use crate::event::{Event, Outgoing};
use crate::message::Message;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use olc_core::Hash;

const PART: usize = 1024;
const MOST_PRUNED_PEERS: usize = 16;
const ASK_AGAIN_MS: u64 = 2_000;
const GIVE_UP_MS: u64 = 30_000;
/// A peer that stops announcing stops counting.
const QUIET_MS: u64 = 15_000;
/// More than any snapshot a board can hold.
const MOST_SNAPSHOT: u64 = 8 * 1024 * 1024;

#[derive(Default)]
pub struct History {
    /// Peers that said which blocks they no longer hold.
    pruned: BTreeMap<PeerId, u64>,
    stuck: Option<(PeerId, u64, u64)>,
}

impl History {
    /// `peer` says `oldest` is its first block. True the first time this leaves the node stuck.
    pub fn pruned(&mut self, peer: PeerId, oldest: u64, now: u64) -> bool {
        if self.pruned.len() >= MOST_PRUNED_PEERS && !self.pruned.contains_key(&peer) {
            self.pruned.pop_first();
        }
        self.pruned.insert(peer, oldest);
        let first_time = self.stuck.is_none();
        let height = self.stuck.map_or(oldest, |(_, height, _)| height);
        self.stuck = Some((peer, height, now));
        first_time
    }

    /// True when `peer` cannot give a node at `own_height` what it needs. Remembers that.
    pub fn lacks(&mut self, peer: PeerId, own_height: u64, peer_height: u64, now: u64) -> bool {
        let lacks = self.pruned.get(&peer).is_some_and(|oldest| *oldest > own_height + 1);
        if lacks {
            self.stuck = Some((peer, peer_height, now));
        }
        lacks
    }

    pub fn tick(&mut self, own_height: u64, now: u64) {
        let over = |(_, height, heard): &(PeerId, u64, u64)| {
            own_height >= *height || now.saturating_sub(*heard) >= QUIET_MS
        };
        if self.stuck.as_ref().is_some_and(over) {
            self.stuck = None;
        }
    }

    pub fn stuck(&self) -> Option<(PeerId, u64)> {
        self.stuck.map(|(peer, height, _)| (peer, height))
    }
}

pub struct Fetching {
    peer: PeerId,
    bytes: Vec<u8>,
    next: u32,
    parts: u32,
    sum: Option<Hash>,
    asked_ms: u64,
    heard_ms: u64,
}

impl Fetching {
    pub fn progress(&self) -> (u32, u32) {
        (self.next, self.parts)
    }
}

impl Engine {
    /// Starts taking `peer`'s unspent set. This trusts that peer: only its owner can decide that,
    /// so this runs from a person's command and from nothing else.
    pub fn accept_snapshot(&mut self, peer: PeerId) {
        self.fetching = Some(Fetching {
            peer,
            bytes: Vec::new(),
            next: 0,
            parts: 0,
            sum: None,
            asked_ms: self.now_ms,
            heard_ms: self.now_ms,
        });
        self.ask_part(peer, 0);
    }

    fn ask_part(&mut self, peer: PeerId, part: u32) {
        let message = Message::GetSnapshot { part };
        self.outbox.push_back(Outgoing::To(peer, message));
    }

    pub(crate) fn keep_fetching(&mut self) {
        let now = self.now_ms;
        let Some(fetching) = self.fetching.as_mut() else {
            return;
        };
        let (peer, next) = (fetching.peer, fetching.next);
        if now.saturating_sub(fetching.heard_ms) >= GIVE_UP_MS {
            self.fetching = None;
            self.push_event(Event::SnapshotRefused { peer });
        } else if now.saturating_sub(fetching.asked_ms) >= ASK_AGAIN_MS {
            fetching.asked_ms = now;
            self.ask_part(peer, next);
        }
    }

    pub(crate) fn serve_snapshot(&mut self, to: PeerId, part: u32) {
        if part == 0 || self.serving.is_none() {
            let bytes = self.chain.snapshot();
            let sum = olc_crypto::hash(&bytes);
            self.serving = Some((bytes, sum));
        }
        let height = self.chain.height();
        let Some((bytes, sum)) = &self.serving else {
            return;
        };
        let parts = bytes.len().div_ceil(PART) as u32;
        let Some(chunk) = bytes.chunks(PART).nth(part as usize) else {
            return;
        };
        let message = Message::SnapshotPart {
            height,
            sum: *sum,
            part,
            parts,
            bytes: chunk.to_vec(),
        };
        if part + 1 == parts {
            self.serving = None;
        }
        self.outbox.push_back(Outgoing::To(to, message));
    }

    pub(crate) fn snapshot_part(
        &mut self,
        from: PeerId,
        sum: Hash,
        part: u32,
        parts: u32,
        bytes: Vec<u8>,
    ) {
        let now = self.now_ms;
        let Some(fetching) = self.fetching.as_mut().filter(|f| f.peer == from) else {
            return;
        };
        let too_big = u64::from(parts) * PART as u64 > MOST_SNAPSHOT;
        if part != fetching.next || too_big || *fetching.sum.get_or_insert(sum) != sum {
            return;
        }
        fetching.bytes.extend_from_slice(&bytes);
        fetching.parts = parts;
        fetching.next += 1;
        fetching.asked_ms = now;
        fetching.heard_ms = now;
        if fetching.next < parts {
            let next = fetching.next;
            self.ask_part(from, next);
            return;
        }
        let bytes = core::mem::take(&mut fetching.bytes);
        self.fetching = None;
        let whole = olc_crypto::hash(&bytes) == sum;
        let adopted = if whole { self.chain.adopt(&bytes, from).ok() } else { None };
        self.candidate = None;
        self.push_event(match adopted {
            Some(height) => Event::SnapshotAdopted { peer: from, height },
            None => Event::SnapshotRefused { peer: from },
        });
    }
}
