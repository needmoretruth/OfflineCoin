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

/// The unspent set being handed out. Building it walks every coin, so one short message from
/// anyone must not start that again. It stays until the askers go quiet.
pub struct Serving {
    tip: Hash,
    height: u64,
    bytes: Vec<u8>,
    sum: Hash,
    asked_ms: u64,
}

/// A node that cannot go on by itself, and what would end that.
#[derive(Clone, Copy)]
struct Stuck {
    peer: PeerId,
    /// How high the peer stands, which this node has to reach for the offer to end.
    ahead: u64,
    heard_ms: u64,
    /// This node's tip when the peer last spoke. The peer's word holds until this moves.
    tip: Hash,
}

#[derive(Default)]
pub struct History {
    /// Peers that said which blocks they no longer hold.
    pruned: BTreeMap<PeerId, u64>,
    stuck: Option<Stuck>,
}

impl History {
    /// `peer` says `oldest` is its first block. True the first time this leaves the node stuck.
    pub fn pruned(&mut self, peer: PeerId, oldest: u64, tip: Hash, now: u64) -> bool {
        if self.pruned.len() >= MOST_PRUNED_PEERS && !self.pruned.contains_key(&peer) {
            self.pruned.pop_first();
        }
        self.pruned.insert(peer, oldest);
        let first_time = self.stuck.is_none();
        let ahead = self.stuck.map_or(oldest, |stuck| stuck.ahead);
        self.stuck = Some(Stuck { peer, ahead, heard_ms: now, tip });
        first_time
    }

    /// True when `peer` cannot give a node at `own` what it needs. Remembers that.
    pub fn lacks(&mut self, peer: PeerId, own: (u64, Hash), peer_height: u64, now: u64) -> bool {
        let (height, tip) = own;
        let lacks = self.pruned.get(&peer).is_some_and(|oldest| *oldest > height + 1);
        if lacks {
            self.stuck = Some(Stuck { peer, ahead: peer_height, heard_ms: now, tip });
        }
        lacks
    }

    /// A height says nothing while the tip stands still: a node on a branch of its own can be
    /// above a peer's first block and share nothing with it.
    pub fn tick(&mut self, own: (u64, Hash), now: u64) {
        let (height, tip) = own;
        let over = |stuck: &Stuck| {
            (height >= stuck.ahead && tip != stuck.tip)
                || now.saturating_sub(stuck.heard_ms) >= QUIET_MS
        };
        if self.stuck.as_ref().is_some_and(over) {
            self.stuck = None;
        }
    }

    /// The peer that can help, and the oldest block anyone said it still holds. That one number
    /// is what a person reads, whichever message a peer sent last.
    pub fn stuck(&self) -> Option<(PeerId, u64)> {
        let stuck = self.stuck?;
        Some((stuck.peer, self.pruned.values().min().copied().unwrap_or(stuck.ahead)))
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
        let tip = self.chain.tip();
        let stale = self.serving.as_ref().is_none_or(|held| part == 0 && held.tip != tip);
        if stale {
            let bytes = self.chain.snapshot();
            let sum = olc_crypto::hash(&bytes);
            let (height, now) = (self.chain.height(), self.now_ms);
            self.serving = Some(Serving { tip, height, bytes, sum, asked_ms: now });
        }
        let now = self.now_ms;
        let Some(held) = self.serving.as_mut() else {
            return;
        };
        held.asked_ms = now;
        let Some(chunk) = held.bytes.chunks(PART).nth(part as usize) else {
            return;
        };
        let message = Message::SnapshotPart {
            height: held.height,
            sum: held.sum,
            part,
            parts: held.bytes.len().div_ceil(PART) as u32,
            bytes: chunk.to_vec(),
        };
        self.outbox.push_back(Outgoing::To(to, message));
    }

    /// Gives the memory back once nobody is taking the snapshot any more.
    pub(crate) fn drop_quiet_snapshot(&mut self) {
        let quiet = |held: &Serving| self.now_ms.saturating_sub(held.asked_ms) >= QUIET_MS;
        if self.serving.as_ref().is_some_and(quiet) {
            self.serving = None;
        }
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
        let held = fetching.bytes.len().saturating_add(bytes.len());
        if part != fetching.next || *fetching.sum.get_or_insert(sum) != sum {
            return;
        }
        if u64::from(parts) * PART as u64 > MOST_SNAPSHOT || held as u64 > MOST_SNAPSHOT {
            self.fetching = None;
            self.push_event(Event::SnapshotRefused { peer: from });
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

#[cfg(test)]
mod tests {
    use super::*;
    use olc_core::{CoinConfig, Plugins};
    use olc_wallet::Wallet;

    fn node() -> Engine {
        let mut coin = CoinConfig::olc();
        coin.initial_difficulty = 8;
        let plugins = Plugins {
            consensus: alloc::boxed::Box::new(olc_pow::ProofOfWork),
            languages: olc_lang::all(),
        };
        Engine::new(coin, plugins, Wallet::from_secret([1; 32]).expect("a good key"))
    }

    /// Where the held snapshot lies. A new one is built while the old one is still held.
    fn held_at(node: &Engine) -> Option<*const u8> {
        node.serving.as_ref().map(|held| held.bytes.as_ptr())
    }

    /// Building the set walks everything a node holds, and asking takes one short message.
    #[test]
    fn asking_again_builds_nothing_new_until_the_chain_moved() {
        let mut node = node();
        node.tick(1_000);
        node.serve_snapshot(5, 0);
        let first = held_at(&node);
        assert!(first.is_some());

        node.tick(2_000);
        node.serve_snapshot(5, 0);
        node.serve_snapshot(5, 1);
        node.serve_snapshot(5, 0);
        assert_eq!(held_at(&node), first, "the same chain, the same snapshot");

        assert!(node.mine(1_000_000, 3_000));
        node.tick(3_000);
        node.serve_snapshot(5, 0);
        assert_ne!(held_at(&node), first);
    }

    #[test]
    fn the_last_part_says_nothing_about_the_next_ask() {
        let mut node = node();
        node.tick(1_000);
        node.serve_snapshot(5, 0);
        let first = held_at(&node);
        let parts = node.chain.snapshot().len().div_ceil(PART) as u32;
        node.serve_snapshot(5, parts - 1);
        node.tick(2_000);
        node.serve_snapshot(5, parts - 1);
        node.serve_snapshot(5, u32::MAX);
        assert_eq!(held_at(&node), first);
    }

    #[test]
    fn a_snapshot_nobody_takes_gives_its_memory_back() {
        let mut node = node();
        node.tick(1_000);
        node.serve_snapshot(5, 0);
        node.tick(1_000 + QUIET_MS);
        assert!(node.serving.is_none());
    }
}
