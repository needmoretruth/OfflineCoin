//! The data on the chain, shaped like early Bitcoin: transactions spend earlier outputs and a
//! block commits to its transactions through a Merkle root.
use alloc::vec::Vec;
use olc_crypto::{hash, Hash};
use serde::{Deserialize, Serialize};

pub use crate::sighash::{ALL, ANYONE_CAN_PAY, NONE, SINGLE};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct OutPoint {
    pub txid: Hash,
    pub index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TxIn {
    pub previous: OutPoint,
    /// Proof for the lock on the output being spent. Its format belongs to the lock language.
    pub unlock: Vec<u8>,
    /// Bitcoin's nSequence, from [`UNLOCK_FREE_VERSION`]: below [`SEQUENCE_DISABLE`] it is a
    /// relative lock in blocks (BIP 68). An older transaction has none and reads as
    /// [`SEQUENCE_FINAL`].
    pub sequence: u32,
}

/// An input that asks for nothing: no relative lock, and it does not hold back the lock time.
pub const SEQUENCE_FINAL: u32 = u32::MAX;
/// Set in a sequence that is no relative lock (BIP 68).
pub const SEQUENCE_DISABLE: u32 = 1 << 31;
/// Set in a sequence that counts time rather than blocks (BIP 68). Boards keep no clock, so OLC
/// counts blocks only, and such a sequence locks nothing.
pub const SEQUENCE_BY_TIME: u32 = 1 << 22;
/// The blocks of a relative lock (BIP 68).
pub const SEQUENCE_BLOCKS: u32 = 0xFFFF;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TxOut {
    pub value: u64,
    /// Which lock language reads `lock`.
    pub language: u8,
    pub lock: Vec<u8>,
}

/// The language byte of a data output. No language has it, so under the existing rules nothing
/// can ever spend such an output, and no node needs to remember one.
pub const DATA_LANGUAGE: u8 = 0xFF;
/// The language of a plain payment, whose lock is an address.
pub const ADDRESS_LANGUAGE: u8 = 0;

/// Starts a signature hash that names a coin. Every transaction starts with its version, and these
/// five bytes are no 32-bit number in that form, so no signature made for no coin can pass as one
/// made for a coin, or the other way round.
pub const COIN_SIGNATURE_START: [u8; 5] = [0xFF; 5];

/// The first transaction version named without its unlock data. A coin takes it from its upgrade
/// height, and a block below that height holding one is refused.
pub const UNLOCK_FREE_VERSION: u32 = 2;

/// The version a wallet writes for a block whose signatures name `coin`: both start at the
/// upgrade height.
pub fn version_for(coin: Option<&Hash>) -> u32 {
    match coin {
        Some(_) => UNLOCK_FREE_VERSION,
        None => 1,
    }
}

impl TxOut {
    /// An output that carries bytes and no value, like Bitcoin's OP_RETURN.
    pub fn data(bytes: Vec<u8>) -> Self {
        Self {
            value: 0,
            language: DATA_LANGUAGE,
            lock: bytes,
        }
    }

    pub fn is_data(&self) -> bool {
        self.language == DATA_LANGUAGE
    }

    /// True for a plain payment to `address`. Any other language may hold the same bytes by
    /// chance or on purpose, and pays nobody by address.
    pub fn pays(&self, address: &[u8]) -> bool {
        self.language == ADDRESS_LANGUAGE && self.lock == address
    }
}

/// Written as [`crate::tx_form`] says: a version 1 transaction as it always was.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tx {
    pub version: u32,
    pub inputs: Vec<TxIn>,
    pub outputs: Vec<TxOut>,
    /// A coinbase has no inputs and carries its block height here, which keeps its id unique.
    pub coinbase_height: Option<u64>,
    /// Bitcoin's nLockTime as a block height, from [`UNLOCK_FREE_VERSION`]: a block below it or at
    /// it cannot hold the transaction unless every input is [`SEQUENCE_FINAL`]. Bitcoin's other
    /// reading, a time, needs a clock boards do not keep, so OLC does not take it.
    pub lock_time: u32,
}

impl Tx {
    pub fn is_coinbase(&self) -> bool {
        self.inputs.is_empty()
    }

    pub fn encode(&self) -> Vec<u8> {
        postcard::to_allocvec(self).unwrap_or_default()
    }

    /// The name outputs are spent by. From [`UNLOCK_FREE_VERSION`] it leaves the unlock data out,
    /// as SegWit's txid does (BIP 141): whoever reshapes how a waiting transaction unlocks cannot
    /// change its name, so a payment signed on top of it before it is mined stays good. An older
    /// version is named by every byte, as it always was.
    pub fn id(&self) -> Hash {
        match self.version >= UNLOCK_FREE_VERSION {
            true => hash(&self.without_unlocks().encode()),
            false => self.whole_id(),
        }
    }

    /// The hash of every byte, unlock data included. A block's Merkle tree is built from these,
    /// so it still commits to every unlock. For an older version it is the id.
    pub fn whole_id(&self) -> Hash {
        hash(&self.encode())
    }

    pub(crate) fn without_unlocks(&self) -> Self {
        let mut blank = self.clone();
        for entry in &mut blank.inputs {
            entry.unlock.clear();
        }
        blank
    }

    /// May a block at `height` hold it, by its lock time? (Bitcoin's IsFinalTx, by height.)
    pub fn final_at(&self, height: u64) -> bool {
        let unlocked = self.lock_time == 0 || u64::from(self.lock_time) < height;
        unlocked || self.inputs.iter().all(|input| input.sequence == SEQUENCE_FINAL)
    }

    /// How many blocks the coin an input spends must have stood, by its sequence (BIP 68). `None`
    /// when it asks for none.
    pub fn relative_lock(&self, input: usize) -> Option<u64> {
        let sequence = self.inputs.get(input)?.sequence;
        let none = self.version < UNLOCK_FREE_VERSION
            || sequence & SEQUENCE_DISABLE != 0
            || sequence & SEQUENCE_BY_TIME != 0;
        (!none).then_some(u64::from(sequence & SEQUENCE_BLOCKS))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockHeader {
    pub version: u32,
    pub previous: Hash,
    pub merkle_root: Hash,
    pub time_ms: u64,
    /// Expected number of attempts to seal this block.
    pub difficulty: u64,
    pub nonce: u32,
}

impl BlockHeader {
    pub const ENCODED_LEN: usize = 88;

    /// A fixed layout, so a miner can change the nonce without encoding the header again.
    pub fn encode(&self) -> [u8; Self::ENCODED_LEN] {
        let mut out = [0u8; Self::ENCODED_LEN];
        out[0..4].copy_from_slice(&self.version.to_le_bytes());
        out[4..36].copy_from_slice(&self.previous);
        out[36..68].copy_from_slice(&self.merkle_root);
        out[68..76].copy_from_slice(&self.time_ms.to_le_bytes());
        out[76..84].copy_from_slice(&self.difficulty.to_le_bytes());
        out[84..88].copy_from_slice(&self.nonce.to_le_bytes());
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let word = |at: usize| bytes.get(at..at + 8)?.try_into().ok().map(u64::from_le_bytes);
        let mut previous = [0; 32];
        let mut merkle_root = [0; 32];
        previous.copy_from_slice(bytes.get(4..36)?);
        merkle_root.copy_from_slice(bytes.get(36..68)?);
        Some(Self {
            version: u32::from_le_bytes(bytes.get(0..4)?.try_into().ok()?),
            previous,
            merkle_root,
            time_ms: word(68)?,
            difficulty: word(76)?,
            nonce: u32::from_le_bytes(bytes.get(84..88)?.try_into().ok()?),
        })
    }

    pub fn id(&self) -> Hash {
        hash(&self.encode())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Block {
    pub header: BlockHeader,
    pub txs: Vec<Tx>,
}

impl Block {
    pub fn id(&self) -> Hash {
        self.header.id()
    }

    pub fn size(&self) -> usize {
        postcard::to_allocvec(self)
            .map(|bytes| bytes.len())
            .unwrap_or(usize::MAX)
    }
}

/// One step up a Merkle tree.
pub(crate) fn pair_hash(left: &Hash, right: &Hash) -> Hash {
    let mut data = [0u8; 64];
    data[..32].copy_from_slice(left);
    data[32..].copy_from_slice(right);
    hash(&data)
}

/// Bitcoin's Merkle tree: hash pairs upward and repeat the last entry of an odd level.
pub fn merkle_root(txs: &[Tx]) -> Hash {
    merkle_tree(txs).0
}

/// The Merkle root, and whether two equal entries sat side by side on the way up. Repeating the
/// last entry means [a, b, c] and [a, b, c, c] share one root and so one block id. A valid block
/// never holds a payment twice, so such a list is a copy made to pass for the real block.
pub fn merkle_tree(txs: &[Tx]) -> (Hash, bool) {
    let mut level: Vec<Hash> = txs.iter().map(Tx::whole_id).collect();
    if level.is_empty() {
        return ([0; 32], false);
    }
    let mut repeated = false;
    while level.len() > 1 {
        repeated |= level.as_chunks::<2>().0.iter().any(|[left, right]| left == right);
        if level.len() % 2 == 1 {
            level.push(level[level.len() - 1]);
        }
        level = level.chunks(2).map(|two| pair_hash(&two[0], &two[1])).collect();
    }
    (level[0], repeated)
}
