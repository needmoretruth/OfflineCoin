//! The two sockets of the chain. A new consensus or lock language is a new implementation of one
//! of these traits, added at start-up, with no change to this crate.
use crate::types::{BlockHeader, Tx, SEQUENCE_FINAL};
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

/// What per-block adjustment may look at, in the spec's terms: an anchor block, which is the
/// parent unless late blocks at the floor came after it.
pub struct PerBlockInput {
    pub anchor_difficulty: u64,
    /// From the anchor's parent's time to the parent's. Below zero when it is earlier, which the
    /// median time past allows.
    pub time_delta_ms: i64,
    /// Blocks from the anchor to the parent: 0 when the parent is the anchor.
    pub height_delta: u64,
    pub block_time_ms: u64,
    pub half_life_ms: u64,
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
    /// The difficulty after a retarget window, under Bitcoin's rule and BIP 94.
    fn next_difficulty(&self, input: &RetargetInput) -> u64;
    /// The difficulty after one block, under per-block adjustment.
    fn per_block_difficulty(&self, input: &PerBlockInput) -> u64;
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
pub struct SpendContext<'a> {
    /// The digest a signature without a kind byte covers.
    pub signature_hash: Hash,
    /// Height of the block spending the output.
    pub height: u64,
    /// The spend itself, from the coin's upgrade on, for what newer words read. `None` before
    /// it, so nothing judged there can depend on it.
    pub spending: Option<Spending<'a>>,
}

/// One input of a transaction on an upgraded coin.
#[derive(Clone, Copy)]
pub struct Spending<'a> {
    pub tx: &'a Tx,
    pub input: usize,
    /// The lock of the coin it spends.
    pub lock: &'a [u8],
    /// What signatures name. See [`crate::CoinConfig::signing_id_at`].
    pub coin: Hash,
}

impl Spending<'_> {
    /// What a signature of `kind` covers. See [`crate::sighash`].
    pub fn signature_hash(&self, kind: u8) -> Option<Hash> {
        self.tx.signature_hash_as(self.input, self.lock, &self.coin, kind)
    }

    /// The input's sequence (BIP 68).
    pub fn sequence(&self) -> u32 {
        self.tx.inputs.get(self.input).map_or(SEQUENCE_FINAL, |input| input.sequence)
    }
}

/// Decides when a locked output can be spent.
pub trait LockLanguage: Send {
    fn id(&self) -> u8;
    fn name(&self) -> &'static str;
    fn check(&self, lock: &[u8], unlock: &[u8], context: &SpendContext) -> bool;
    /// The most signature checks a spend can cost, read without running it. A node's pool reads
    /// this. No block is ever judged by it.
    fn signature_checks(&self, _lock: &[u8], _unlock: &[u8]) -> usize {
        1
    }
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
