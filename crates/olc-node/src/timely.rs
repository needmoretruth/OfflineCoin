//! The node's clock against the chain's: the time a block it mines carries, and a block too far
//! ahead of a clock that knows the date, which waits until it is not.
use crate::engine::Engine;
use olc_core::timing::DATED_CLOCK_FROM_MS;
use olc_core::{Block, BlockHeader};

impl Engine {
    /// True when the device clock counts from the date, as a computer's does. A board's counts
    /// from its own start.
    pub(crate) fn clock_is_dated(&self) -> bool {
        self.now_ms >= DATED_CLOCK_FROM_MS
    }

    /// The time now, as blocks count it.
    pub(crate) fn chain_now(&self, device_ms: u64) -> u64 {
        device_ms.saturating_add(self.clock_offset_ms)
    }

    /// A board has no clock of the date, so it runs at least as late as its chain's tip. A
    /// computer keeps its own: a block stamped ahead must not pull it along, or a few such
    /// blocks would carry every clock as far ahead as anyone liked.
    pub(crate) fn follow_chain_clock(&mut self, device_ms: u64) {
        if device_ms >= DATED_CLOCK_FROM_MS {
            self.clock_offset_ms = 0;
            return;
        }
        let chain_ms = self.chain.tip_header().time_ms;
        self.clock_offset_ms = self.clock_offset_ms.max(chain_ms.saturating_sub(device_ms));
    }

    /// True when `block` is further ahead of this clock than its coin allows. As Bitcoin Core's
    /// `time-too-new`, it is not taken now and not held against anyone: sent again once its time
    /// is near, it is taken.
    pub(crate) fn is_from_the_future(&self, block: &Block) -> bool {
        if !self.clock_is_dated() {
            return false;
        }
        let height = block.txs.first().and_then(|tx| tx.coinbase_height).unwrap_or(0);
        let ahead = self.chain.config().rules_at(height).max_ahead_ms;
        ahead > 0 && block.header.time_ms > self.now_ms.saturating_add(ahead)
    }

    /// True when `header` was timed before a late block's floor came and now a header may take
    /// it, so it should be timed again at once. `now` is the chain's time.
    pub(crate) fn floor_has_come(&self, header: &BlockHeader, now: u64) -> bool {
        let late = self.chain.floor_after(&self.chain.tip());
        late.is_some_and(|late| header.time_ms <= late && self.stamp(now) > late)
    }

    /// As [`Engine::floor_has_come`], for a miner that holds its own copy of the header on the
    /// tip, at `device_ms` by its clock.
    pub fn floor_came(&self, header: &BlockHeader, device_ms: u64) -> bool {
        self.floor_has_come(header, self.chain_now(device_ms))
    }

    /// `now` as a header carries it: whole seconds on a coin with Bitcoin's header.
    fn stamp(&self, now: u64) -> u64 {
        match self.chain.config().bitcoin_header {
            true => now / 1_000 * 1_000,
            false => now,
        }
    }

    /// `header` on the tip as it may be at `now`, the chain's time: that time in whole seconds on
    /// a coin with Bitcoin's header, never before the earliest time a block may carry nor before
    /// its own, and the difficulty that time asks. A header that stays the same keeps its nonce,
    /// or the nonces tried would be tried again.
    fn timed(&self, header: &BlockHeader, now: u64) -> BlockHeader {
        let tip = self.chain.tip();
        let time_ms = self.stamp(now).max(header.time_ms).max(self.chain.earliest_time(&tip));
        let difficulty = self.chain.next_difficulty_at(&tip, time_ms);
        let mut timed = *header;
        if (timed.time_ms, timed.difficulty) != (time_ms, difficulty) {
            (timed.time_ms, timed.difficulty, timed.nonce) = (time_ms, difficulty, 0);
        }
        timed
    }

    /// As the candidate is timed again, for a miner that holds its own copy of the header on the
    /// tip, at `device_ms` by its clock. A header given a time by hand may break the rules: a
    /// time the coin cannot carry, or one that asks another difficulty.
    pub fn retimed(&self, header: &BlockHeader, device_ms: u64) -> BlockHeader {
        self.timed(header, self.chain_now(device_ms))
    }

    /// Moves the candidate's time to `now`, the chain's time, and gives it the difficulty that
    /// time asks.
    pub(crate) fn retime_candidate(&mut self, now: u64) {
        let Some(timed) = self.candidate.as_ref().map(|block| self.timed(&block.header, now)) else {
            return;
        };
        if let Some(block) = self.candidate.as_mut() {
            block.header = timed;
        }
    }

    /// The difficulty the next block asks if it is found now.
    pub(crate) fn difficulty_now(&self) -> u64 {
        let tip = self.chain.tip();
        let now = self.chain_now(self.now_ms).max(self.chain.earliest_time(&tip));
        self.chain.next_difficulty_at(&tip, now)
    }
}
