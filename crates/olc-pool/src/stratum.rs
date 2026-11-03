//! Stratum v1 as Bitcoin pools speak it (slushpool's 2012 text, en.bitcoin.it/wiki/Stratum_mining_protocol):
//! the shapes of `mining.notify` and `mining.submit`, and the header a miner builds from them.
//! Both ends of OLC's pool use these, and a miner built for Bitcoin builds the same header.
use olc_core::bits::{bits_of, difficulty_of, target_of, Wide};
use olc_core::{coinbase_split, Block, BlockHeader, Hash, EXTRANONCE_BYTES};
use olc_crypto::hash;
use serde_json::{json, Value};

/// The part of the coinbase's extranonce a pool gives each connection.
pub const EXTRANONCE1_BYTES: usize = 4;
/// The part a miner rolls.
pub const EXTRANONCE2_BYTES: usize = EXTRANONCE_BYTES - EXTRANONCE1_BYTES;
/// The version bits a miner may roll (BIP 320), which no rule reads.
pub const VERSION_ROLLING_MASK: u32 = olc_core::types::ROLLED_BITS;

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Hex of even length and nothing else.
pub fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    (0..text.len() / 2)
        .map(|at| u8::from_str_radix(text.get(at * 2..at * 2 + 2)?, 16).ok())
        .collect()
}

/// A number as Stratum writes version, bits, time and nonce: eight hex digits, big-endian.
pub fn word_hex(word: u32) -> String {
    format!("{word:08x}")
}

pub fn read_word(text: &str) -> Option<u32> {
    (text.len() == 8).then(|| u32::from_str_radix(text, 16).ok()).flatten()
}

/// The previous block's id as `mining.notify` carries it: the header's bytes with each four-byte
/// word turned around. Turning them again gives the header's bytes back.
pub fn swap_words(bytes: &[u8; 32]) -> [u8; 32] {
    let mut out = *bytes;
    for word in out.chunks_mut(4) {
        word.reverse();
    }
    out
}

/// The hashes beside the coinbase on its way up Bitcoin's Merkle tree, lowest first.
pub fn coinbase_branch(leaves: &[Hash]) -> Vec<Hash> {
    let mut branch = Vec::new();
    let mut level: Vec<Hash> = leaves.to_vec();
    while level.len() > 1 {
        if level.len() % 2 == 1 {
            level.push(level[level.len() - 1]);
        }
        branch.push(level[1]);
        level = level.chunks(2).map(|two| pair(&two[0], &two[1])).collect();
    }
    branch
}

fn pair(left: &Hash, right: &Hash) -> Hash {
    let mut data = [0u8; 64];
    data[..32].copy_from_slice(left);
    data[32..].copy_from_slice(right);
    hash(&data)
}

pub fn root_from_branch(leaf: Hash, branch: &[Hash]) -> Hash {
    branch.iter().fold(leaf, |root, beside| pair(&root, beside))
}

/// Bitcoin's difficulty 1 target, `0xFFFF × 2^208`, which a Stratum difficulty divides.
fn difficulty_one() -> f64 {
    65_535.0 * 2f64.powi(208)
}

fn as_float(target: &Wide) -> f64 {
    target.iter().rev().fold(0.0, |sum, word| sum * 18_446_744_073_709_551_616.0 + *word as f64)
}

/// The Stratum difficulty that stands for OLC's: the hashes a share takes over 2^32, read off
/// the same compact target. A share can be easier than Bitcoin's difficulty 1, so it is a fraction.
pub fn stratum_difficulty(difficulty: u64) -> f64 {
    let target = target_of(bits_of(difficulty)).unwrap_or([u64::MAX; 4]);
    difficulty_one() / as_float(&target).max(1.0)
}

/// OLC's difficulty for a Stratum one, rounded up so that a share found at it always meets the
/// pool's target: a hair more work than asked, never less.
pub fn olc_difficulty(stratum: f64) -> u64 {
    if !stratum.is_finite() || stratum <= 0.0 {
        return 1;
    }
    let hashes = stratum * 281_474_976_710_656.0 / 65_535.0 * (1.0 + 1e-9);
    let wanted = hashes.ceil().clamp(1.0, u64::MAX as f64) as u64;
    difficulty_of(bits_of(wanted))
}

/// A block to mine, as `mining.notify` sends it.
#[derive(Clone, Debug, PartialEq)]
pub struct Notify {
    pub job_id: String,
    /// The previous block's id in header order.
    pub previous: Hash,
    pub coinb1: Vec<u8>,
    pub coinb2: Vec<u8>,
    pub branch: Vec<Hash>,
    pub version: u32,
    pub bits: u32,
    pub ntime: u32,
    pub clean: bool,
}

impl Notify {
    /// The work in `block`, a template whose coinbase ends in an extranonce. `None` for a block
    /// in OLC's first header, which a miner built for Bitcoin cannot hash.
    pub fn of_block(job_id: String, block: &Block, clean: bool) -> Option<Self> {
        let header = &block.header;
        if !header.is_bitcoin_shaped() || !header.is_whole() {
            return None;
        }
        let (coinb1, coinb2) = coinbase_split(block.txs.first()?)?;
        let leaves: Vec<Hash> = block.txs.iter().map(|tx| tx.whole_id()).collect();
        Some(Self {
            job_id,
            previous: header.previous,
            coinb1,
            coinb2,
            branch: coinbase_branch(&leaves),
            version: header.version,
            bits: bits_of(header.difficulty),
            ntime: u32::try_from(header.time_ms / 1_000).ok()?,
            clean,
        })
    }

    pub fn params(&self) -> Value {
        let branch: Vec<String> = self.branch.iter().map(|step| hex(step)).collect();
        json!([
            self.job_id,
            hex(&swap_words(&self.previous)),
            hex(&self.coinb1),
            hex(&self.coinb2),
            branch,
            word_hex(self.version),
            word_hex(self.bits),
            word_hex(self.ntime),
            self.clean
        ])
    }

    pub fn from_params(params: &Value) -> Option<Self> {
        let text = |at: usize| params.get(at)?.as_str();
        let hash_at = |text: &str| -> Option<Hash> { unhex(text)?.try_into().ok() };
        let branch = params.get(4)?.as_array()?;
        let branch: Option<Vec<Hash>> = branch.iter().map(|step| hash_at(step.as_str()?)).collect();
        Some(Self {
            job_id: text(0)?.to_string(),
            previous: swap_words(&hash_at(text(1)?)?),
            coinb1: unhex(text(2)?)?,
            coinb2: unhex(text(3)?)?,
            branch: branch?,
            version: read_word(text(5)?)?,
            bits: read_word(text(6)?)?,
            ntime: read_word(text(7)?)?,
            clean: params.get(8)?.as_bool()?,
        })
    }

    /// The coinbase with `extranonce` in its place, as a miner hashes it.
    pub fn coinbase_bytes(&self, extranonce: &[u8]) -> Vec<u8> {
        [self.coinb1.as_slice(), extranonce, self.coinb2.as_slice()].concat()
    }

    /// The header a miner hashes for these choices. `None` when the bits stand for no target.
    pub fn header(&self, extranonce: &[u8], ntime: u32, nonce: u32, version: u32) -> Option<BlockHeader> {
        target_of(self.bits)?;
        let leaf = hash(&self.coinbase_bytes(extranonce));
        let header = BlockHeader {
            version,
            previous: self.previous,
            merkle_root: root_from_branch(leaf, &self.branch),
            time_ms: u64::from(ntime) * 1_000,
            difficulty: difficulty_of(self.bits),
            nonce,
        };
        (bits_of(header.difficulty) == self.bits).then_some(header)
    }
}

/// A line of JSON-RPC as Stratum frames it: one object, one line.
pub fn line(value: &Value) -> String {
    let mut text = value.to_string();
    text.push('\n');
    text
}

pub fn request(id: u64, method: &str, params: Value) -> String {
    line(&json!({ "id": id, "method": method, "params": params }))
}

pub fn notification(method: &str, params: Value) -> String {
    line(&json!({ "id": Value::Null, "method": method, "params": params }))
}

pub fn answer(id: &Value, result: Value) -> String {
    line(&json!({ "id": id, "result": result, "error": Value::Null }))
}

/// Stratum's error codes, as the 2012 text and every pool since number them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    Other,
    /// The job is gone: the chain moved on, or it was never handed out.
    Stale,
    Duplicate,
    /// The hash does not reach the share's difficulty.
    Weak,
    Unauthorized,
    NotSubscribed,
    /// Fields of the wrong length or shape.
    Malformed,
    /// A time this job does not allow.
    Time,
    /// Version bits outside those agreed to roll.
    Version,
}

impl Refusal {
    pub fn code(self) -> i64 {
        match self {
            Self::Other | Self::Malformed | Self::Time | Self::Version => 20,
            Self::Stale => 21,
            Self::Duplicate => 22,
            Self::Weak => 23,
            Self::Unauthorized => 24,
            Self::NotSubscribed => 25,
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            Self::Other => "Other",
            Self::Stale => "Job not found",
            Self::Duplicate => "Duplicate share",
            Self::Weak => "Low difficulty share",
            Self::Unauthorized => "Unauthorized worker",
            Self::NotSubscribed => "Not subscribed",
            Self::Malformed => "Malformed share",
            Self::Time => "Time out of range",
            Self::Version => "Version bits not allowed",
        }
    }
}

pub fn refused(id: &Value, refusal: Refusal) -> String {
    let error = json!([refusal.code(), refusal.message(), Value::Null]);
    line(&json!({ "id": id, "result": Value::Null, "error": error }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_branch_climbs_to_the_same_root_as_the_whole_tree() {
        for count in 1..9u8 {
            let leaves: Vec<Hash> = (0..count).map(|at| [at; 32]).collect();
            let mut level = leaves.clone();
            while level.len() > 1 {
                if level.len() % 2 == 1 {
                    level.push(level[level.len() - 1]);
                }
                level = level.chunks(2).map(|two| pair(&two[0], &two[1])).collect();
            }
            assert_eq!(root_from_branch(leaves[0], &coinbase_branch(&leaves)), level[0], "{count}");
        }
    }

    #[test]
    fn difficulty_turns_into_stratum_and_back_never_easier() {
        for difficulty in [1, 2, 30_000, 4_294_967_296, 1 << 40, 123_456_789_012] {
            let rounded = difficulty_of(bits_of(difficulty));
            let back = olc_difficulty(stratum_difficulty(difficulty));
            assert!(back >= rounded, "{difficulty}: {back} < {rounded}");
            assert!(back - rounded <= rounded / 4_096 + 1, "{difficulty}: {back} far from {rounded}");
        }
        // Bitcoin's difficulty 1 is 2^32 hashes, as its pools count them.
        let one = stratum_difficulty(0x1_0001_0001);
        assert!((one - 1.0).abs() < 1e-9, "{one}");
        assert_eq!(olc_difficulty(f64::NAN), 1);
        assert_eq!(olc_difficulty(-3.0), 1);
    }

    #[test]
    fn words_read_back_only_from_eight_hex_digits() {
        assert_eq!(read_word(&word_hex(0x2000_0000)), Some(0x2000_0000));
        assert_eq!(read_word("2000000"), None);
        assert_eq!(read_word("2000000g"), None);
        assert_eq!(unhex("abc"), None);
        assert_eq!(unhex("éé"), None);
        assert_eq!(swap_words(&swap_words(&[7; 32])), [7; 32]);
    }
}
