//! The two sockets of the chain. A new consensus or lock language is a new implementation of one
//! of these traits, added at start-up, with no change to this crate.
use crate::types::BlockHeader;
use alloc::boxed::Box;
use alloc::vec::Vec;
use olc_crypto::Hash;

/// What the retarget formula may look at.
pub struct RetargetInput {
    pub parent_difficulty: u64,
    /// Time the last retarget window actually took.
    pub actual_ms: u64,
    /// Time that window should have taken.
    pub expected_ms: u64,
    /// The answer may not go below this.
    pub min_difficulty: u64,
}

/// Decides who may add a block and how much a block counts. PoW is one implementation.
pub trait Consensus: Send {
    fn name(&self) -> &'static str;
    /// Does this header carry a valid proof?
    fn check_seal(&self, header: &BlockHeader) -> bool;
    /// How much this block adds to its chain. The chain with the most total work wins.
    fn work(&self, header: &BlockHeader) -> u128;
    fn next_difficulty(&self, input: &RetargetInput) -> u64;
    /// Tries `attempts` times to seal the header. Returns true when it succeeded.
    fn try_seal(&self, header: &mut BlockHeader, attempts: u32) -> bool {
        let difficulty = header.difficulty;
        self.try_meet(header, difficulty, attempts)
    }
    /// Does this header carry proof of `difficulty`, which may differ from its own? This lets
    /// someone show work on a block that does not win.
    fn meets(&self, header: &BlockHeader, difficulty: u64) -> bool;
    /// Tries `attempts` times to reach `difficulty`. Returns true when it did.
    fn try_meet(&self, header: &mut BlockHeader, difficulty: u64, attempts: u32) -> bool;
}

/// What a lock language may look at when an output is spent.
pub struct SpendContext {
    /// The digest signatures must cover.
    pub signature_hash: Hash,
    /// Height of the block spending the output.
    pub height: u64,
}

/// Decides when a locked output can be spent.
pub trait LockLanguage: Send {
    fn id(&self) -> u8;
    fn name(&self) -> &'static str;
    fn check(&self, lock: &[u8], unlock: &[u8], context: &SpendContext) -> bool;
}

/// Everything a chain needs from outside.
pub struct Plugins {
    pub consensus: Box<dyn Consensus>,
    pub languages: Vec<Box<dyn LockLanguage>>,
}

impl Plugins {
    pub fn language(&self, id: u8) -> Option<&dyn LockLanguage> {
        self.languages
            .iter()
            .find(|language| language.id() == id)
            .map(|boxed| boxed.as_ref())
    }
}
