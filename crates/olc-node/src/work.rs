//! Generic work: grinding a header someone else built to a difficulty they chose. Mining alone
//! never needs this. It is here so a node can lend its hashes without knowing what for.
use crate::engine::Engine;
use crate::event::Event;
use olc_core::{Address, Block, BlockHeader, Tx};

impl Engine {
    /// Builds an unsealed block that pays `address`, for someone who hands out work.
    pub fn work_template(&self, address: &Address, time_ms: u64, note: &[u8]) -> Block {
        let pool: &[Tx] = &self.pool;
        self.chain.block_template_with(address, pool, time_ms, note)
    }

    /// Tries `attempts` nonces on `header`. True when the header now reaches `difficulty`.
    pub fn grind(&self, header: &mut BlockHeader, difficulty: u64, attempts: u32) -> bool {
        let consensus = &self.chain.plugins().consensus;
        consensus.try_meet(header, difficulty, attempts)
    }

    /// Adds a block sealed elsewhere and tells the peers, as if this node had mined it.
    pub fn submit_block(&mut self, block: Block) -> bool {
        let height = self.chain.height() + 1;
        let old_tip = self.chain.tip();
        if self.chain.add_block(block).is_err() {
            return false;
        }
        self.push_event(Event::Mined { height });
        self.after_tip_change(old_tip);
        self.announce(None);
        true
    }

    /// True when `header` reaches `difficulty`.
    pub fn work_is_done(&self, header: &BlockHeader, difficulty: u64) -> bool {
        self.chain.plugins().consensus.meets(header, difficulty)
    }
}
