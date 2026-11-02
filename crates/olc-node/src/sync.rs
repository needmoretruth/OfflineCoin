//! Catching up, as Bitcoin does it: a node asks one peer for the blocks after its own position,
//! and each one connects as it arrives.
use crate::engine::PeerId;
use alloc::vec::Vec;
use olc_core::Hash;

/// What a peer sends for one request. A small radio queue on a slow reader sets this.
pub const BLOCKS_AT_ONCE: usize = 8;
const ASK_AGAIN_MS: u64 = 1_000;
/// Well past a peer's next announcement, so a peer that left stops mattering. A peer that keeps
/// talking but brings no block for this long has claimed work it does not show.
const TARGET_FORGOTTEN_MS: u64 = 15_000;
/// How long such a peer's claims are not followed, so others get their turn.
const DOUBTED_MS: u64 = 60_000;
/// How long a peer that sent an invalid block is not followed. Peers on the same genesis with
/// other rules keep announcing their tips.
const INVALID_MS: u64 = 600_000;
const MOST_DOUBTED: usize = 8;
/// A heavy block can take a board this long, so catching up lasts this long past the last block.
const MOVING_MS: u64 = 5_000;
/// A node this far behind would only mine blocks that are already lost.
const WELL_BEHIND: u64 = 4;

struct Target {
    peer: PeerId,
    height: u64,
    work: u128,
    heard_ms: u64,
    since_ms: u64,
}

#[derive(Default)]
pub struct Sync {
    target: Option<Target>,
    asked_ms: u64,
    expected: usize,
    blocks_came: bool,
    /// When a block from the target last connected. Only this says the claim is real.
    moved_ms: Option<u64>,
    /// Peers that claimed and did not deliver, and until when their word goes unheard.
    doubted: Vec<(PeerId, u64)>,
    /// The newest block that came from the target, which may be on a branch the node has not
    /// moved to yet.
    position: Option<(u64, Hash)>,
}

impl Sync {
    pub fn heard(&mut self, peer: PeerId, height: u64, work: u128, now: u64) {
        self.doubted.retain(|(_, until)| now < *until);
        if self.doubted.iter().any(|(doubted, _)| *doubted == peer) {
            return;
        }
        let same = self.target.as_ref().filter(|target| target.peer == peer);
        let since_ms = same.map_or(now, |target| target.since_ms);
        let replaces = self.target.as_ref().is_none_or(|target| {
            target.peer == peer
                || work >= target.work
                || now.saturating_sub(target.heard_ms) >= TARGET_FORGOTTEN_MS
        });
        if replaces {
            self.target = Some(Target {
                peer,
                height,
                work,
                heard_ms: now,
                since_ms,
            });
        }
    }

    /// Called once a turn, before the blocks of that turn: checking a heavy block can outlast the
    /// wait, and that says nothing about the peer.
    pub fn new_turn(&mut self, now: u64) {
        if core::mem::take(&mut self.blocks_came) {
            self.asked_ms = now;
        }
    }

    /// The peer to ask now, if one is due. Forgets a target that was reached or went quiet.
    pub fn next_ask(&mut self, own_work: u128, now: u64) -> Option<PeerId> {
        let target = self.target.as_ref()?;
        let peer = target.peer;
        if own_work >= target.work || now.saturating_sub(target.heard_ms) >= TARGET_FORGOTTEN_MS {
            self.stop();
            return None;
        }
        let waited_from = target.since_ms.max(self.moved_ms.unwrap_or(0));
        if now.saturating_sub(waited_from) >= TARGET_FORGOTTEN_MS {
            self.doubt(peer, now + DOUBTED_MS);
            return None;
        }
        let due = self.expected == 0 || now.saturating_sub(self.asked_ms) >= ASK_AGAIN_MS;
        due.then(|| {
            self.asked_ms = now;
            self.expected = BLOCKS_AT_ONCE;
            peer
        })
    }

    /// Stops following `peer`, and its claims go unheard until `until`.
    fn doubt(&mut self, peer: PeerId, until: u64) {
        self.stop();
        if self.doubted.len() >= MOST_DOUBTED {
            self.doubted.remove(0);
        }
        self.doubted.push((peer, until));
    }

    /// Everything about the target goes. What was learned about peers stays.
    fn stop(&mut self) {
        let doubted = core::mem::take(&mut self.doubted);
        *self = Self {
            doubted,
            ..Self::default()
        };
    }

    pub fn position(&self) -> Option<(u64, Hash)> {
        self.position
    }

    pub fn is_active(&self) -> bool {
        self.target.is_some()
    }

    /// A requested block arrived. Only one that connected moves the position.
    pub fn arrived(&mut self, connected: Option<(u64, Hash)>, known: bool, now: u64) {
        if !known {
            self.expected = self.expected.saturating_sub(1);
            self.blocks_came = true;
            self.moved_ms = connected.map_or(self.moved_ms, |_| Some(now));
        }
        self.position = connected.or(self.position);
    }

    /// A page of headers arrived, ending at `position`. An empty page says nothing new, so the
    /// next question waits for its turn.
    pub fn answered(&mut self, count: usize, position: Option<(u64, Hash)>, now: u64) {
        if count > 0 {
            self.expected = 0;
            self.moved_ms = Some(now);
        }
        self.position = position;
    }

    /// `peer` cannot give what is needed, so it is no target.
    pub fn drop_peer(&mut self, peer: PeerId) {
        if self.target.as_ref().is_some_and(|target| target.peer == peer) {
            self.stop();
        }
    }

    /// `peer` sent something invalid. The tip it named stays unjudged: anyone can name a true
    /// block, so only the peer loses its turn, for long enough that a rival chain costs little.
    pub fn give_up_on(&mut self, peer: PeerId, now: u64) {
        if self.target.as_ref().is_some_and(|target| target.peer == peer) {
            self.doubt(peer, now + INVALID_MS);
        }
    }

    /// The height being caught up to, while the node is well behind it.
    pub fn behind(&self, height: u64, work: u128, now: u64) -> Option<u64> {
        let target = self.target.as_ref()?;
        let fresh = now.saturating_sub(target.heard_ms) < TARGET_FORGOTTEN_MS;
        (fresh && work < target.work && target.height >= height + WELL_BEHIND)
            .then_some(target.height)
    }

    /// True while blocks are really coming in. A peer's word alone never makes this true.
    pub fn moving(&self, now: u64) -> bool {
        self.moved_ms.is_some_and(|at| now.saturating_sub(at) < MOVING_MS)
    }
}
