//! Catching up, as Bitcoin does it: a node asks one peer for the blocks after its own position,
//! and each one connects as it arrives.
use crate::engine::PeerId;
use olc_core::Hash;

/// What a peer sends for one request. A small radio queue on a slow reader sets this.
pub const BLOCKS_AT_ONCE: usize = 8;
const ASK_AGAIN_MS: u64 = 1_000;
/// Well past a peer's next announcement, so a peer that left stops mattering.
const TARGET_FORGOTTEN_MS: u64 = 15_000;
/// A node this far behind would only mine blocks that are already lost.
const WELL_BEHIND: u64 = 4;

struct Target {
    peer: PeerId,
    tip: Hash,
    height: u64,
    work: u128,
    heard_ms: u64,
}

#[derive(Default)]
pub struct Sync {
    target: Option<Target>,
    asked_ms: u64,
    expected: usize,
    /// The newest block that came from the target, which may be on a branch the node has not
    /// moved to yet.
    position: Option<(u64, Hash)>,
}

impl Sync {
    pub fn heard(&mut self, peer: PeerId, tip: Hash, height: u64, work: u128, now: u64) {
        let replaces = self.target.as_ref().is_none_or(|target| {
            target.peer == peer
                || work >= target.work
                || now.saturating_sub(target.heard_ms) >= TARGET_FORGOTTEN_MS
        });
        if replaces {
            self.target = Some(Target {
                peer,
                tip,
                height,
                work,
                heard_ms: now,
            });
        }
    }

    /// The peer to ask now, if one is due. Forgets a target that was reached or went quiet.
    pub fn next_ask(&mut self, own_work: u128, now: u64) -> Option<PeerId> {
        let target = self.target.as_ref()?;
        if own_work >= target.work || now.saturating_sub(target.heard_ms) >= TARGET_FORGOTTEN_MS {
            *self = Self::default();
            return None;
        }
        let peer = target.peer;
        let due = self.expected == 0 || now.saturating_sub(self.asked_ms) >= ASK_AGAIN_MS;
        due.then(|| {
            self.asked_ms = now;
            self.expected = BLOCKS_AT_ONCE;
            peer
        })
    }

    pub fn position(&self) -> Option<(u64, Hash)> {
        self.position
    }

    pub fn is_active(&self) -> bool {
        self.target.is_some()
    }

    /// A requested block arrived. Only one that connected moves the position.
    pub fn arrived(&mut self, connected: Option<(u64, Hash)>) {
        self.expected = self.expected.saturating_sub(1);
        self.position = connected.or(self.position);
    }

    /// `peer` cannot give what is needed, so it is no target.
    pub fn drop_peer(&mut self, peer: PeerId) {
        if self.target.as_ref().is_some_and(|target| target.peer == peer) {
            *self = Self::default();
        }
    }

    /// `peer` sent something invalid. Returns its tip, which descends from it.
    pub fn give_up_on(&mut self, peer: PeerId) -> Option<Hash> {
        let tip = self.target.as_ref().filter(|t| t.peer == peer)?.tip;
        *self = Self::default();
        Some(tip)
    }

    /// The height being caught up to, while the node is well behind it.
    pub fn behind(&self, height: u64, work: u128, now: u64) -> Option<u64> {
        let target = self.target.as_ref()?;
        let fresh = now.saturating_sub(target.heard_ms) < TARGET_FORGOTTEN_MS;
        (fresh && work < target.work && target.height >= height + WELL_BEHIND)
            .then_some(target.height)
    }
}
