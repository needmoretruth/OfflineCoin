//! The data on the chain, shaped like early Bitcoin: transactions spend earlier outputs and a
//! block commits to its transactions through a Merkle root.
use alloc::vec::Vec;
use olc_crypto::{hash, Hash};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct OutPoint {
    pub txid: Hash,
    pub index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TxIn {
    pub previous: OutPoint,
    /// Proof for the lock on the output being spent. Its format belongs to the lock language.
    pub unlock: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TxOut {
    pub value: u64,
    /// Which lock language reads `lock`.
    pub language: u8,
    pub lock: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tx {
    pub version: u32,
    pub inputs: Vec<TxIn>,
    pub outputs: Vec<TxOut>,
    /// A coinbase has no inputs and carries its block height here, which keeps its id unique.
    pub coinbase_height: Option<u64>,
}

impl Tx {
    pub fn is_coinbase(&self) -> bool {
        self.inputs.is_empty()
    }

    pub fn encode(&self) -> Vec<u8> {
        postcard::to_allocvec(self).unwrap_or_default()
    }

    pub fn id(&self) -> Hash {
        hash(&self.encode())
    }

    /// What a signature commits to: the transaction without its unlock data, the input being
    /// signed and the lock it spends.
    pub fn signature_hash(&self, input: usize, lock: &[u8]) -> Hash {
        let mut blank = self.clone();
        for entry in &mut blank.inputs {
            entry.unlock.clear();
        }
        let mut data = blank.encode();
        data.extend_from_slice(&(input as u32).to_le_bytes());
        data.extend_from_slice(lock);
        hash(&data)
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

/// Bitcoin's Merkle tree: hash pairs upward and repeat the last entry of an odd level.
pub fn merkle_root(txs: &[Tx]) -> Hash {
    let mut level: Vec<Hash> = txs.iter().map(Tx::id).collect();
    if level.is_empty() {
        return [0; 32];
    }
    while level.len() > 1 {
        if level.len() % 2 == 1 {
            level.push(level[level.len() - 1]);
        }
        level = level
            .chunks(2)
            .map(|pair| {
                let mut data = [0u8; 64];
                data[..32].copy_from_slice(&pair[0]);
                data[32..].copy_from_slice(&pair[1]);
                hash(&data)
            })
            .collect();
    }
    level[0]
}
