//! The node's clock against the chain's: the time a block it mines carries, and a block too far
//! ahead of a clock that knows the date, which waits until it is not.
use crate::engine::Engine;
use olc_core::timing::DATED_CLOCK_FROM_MS;
use olc_core::{Block, DifficultyRule};

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
    pub(crate) fn from_the_future(&self, block: &Block) -> bool {
        if !self.clock_is_dated() {
            return false;
        }
        let height = block.txs.first().and_then(|tx| tx.coinbase_height).unwrap_or(0);
        let ahead = self.chain.config().rules_at(height).max_ahead_ms;
        ahead > 0 && block.header.time_ms > self.now_ms.saturating_add(ahead)
    }

    /// True when, under BIP 94, the candidate was built before its parent was two block times
    /// old and now it is, so the floor has come and the candidate should take it at once.
    pub(crate) fn floor_has_come(&self, block: &Block, now: u64) -> bool {
        let height = self.chain.height() + 1;
        let rules = self.chain.config().rules_at(height);
        let between = rules.retarget_interval > 0 && !height.is_multiple_of(rules.retarget_interval);
        let late = self.chain.tip_header().time_ms.saturating_add(rules.block_time_ms.saturating_mul(2));
        rules.difficulty_rule == DifficultyRule::Bip94 && between && block.header.time_ms <= late && now > late
    }

    /// Moves the candidate's time to `now`, in whole seconds on a coin with Bitcoin's header,
    /// and gives it the difficulty that time asks.
    pub(crate) fn retime_candidate(&mut self, now: u64) {
        let tip = self.chain.tip();
        let Some(time_ms) = self.candidate.as_ref().map(|block| block.header.time_ms) else {
            return;
        };
        let whole = match self.chain.config().bitcoin_header {
            true => now / 1_000 * 1_000,
            false => now,
        };
        let time_ms = whole.max(time_ms).max(self.chain.earliest_time(&tip));
        let difficulty = self.chain.next_difficulty_at(&tip, time_ms);
        if let Some(block) = self.candidate.as_mut() {
            block.header.time_ms = time_ms;
            block.header.difficulty = difficulty;
            block.header.nonce = 0;
        }
    }

    /// The difficulty the next block asks if it is found now.
    pub(crate) fn difficulty_now(&self) -> u64 {
        let tip = self.chain.tip();
        let now = self.chain_now(self.now_ms).max(self.chain.earliest_time(&tip));
        self.chain.next_difficulty_at(&tip, now)
    }
}
