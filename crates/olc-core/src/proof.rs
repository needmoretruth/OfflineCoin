//! What a node without blocks can still check, as in section 8 of the Bitcoin paper: a Merkle
//! branch ties a transaction to a header. The tree itself is the one in every block.
use crate::chain::Chain;
use crate::ledger::Unspent;
use crate::types::{pair_hash, Block, BlockHeader, OutPoint, Tx};
use alloc::vec::Vec;
use olc_crypto::{Address, Hash};
use serde::{Deserialize, Serialize};

/// Deeper than any block can be, so a longer branch is a lie.
const LONGEST_BRANCH: usize = 32;
/// Two hashes side by side are this long. A transaction of this size could pass for an inner node
/// of the tree, the known weakness of Bitcoin's Merkle proofs.
const INNER_NODE_BYTES: usize = 64;

/// The hashes beside `position` on its way up to the root.
pub fn merkle_branch(txs: &[Tx], position: usize) -> Vec<Hash> {
    let mut level: Vec<Hash> = txs.iter().map(Tx::id).collect();
    let mut at = position;
    let mut branch = Vec::new();
    while level.len() > 1 {
        if level.len() % 2 == 1 {
            level.push(level[level.len() - 1]);
        }
        branch.push(level[at ^ 1]);
        level = level.chunks(2).map(|two| pair_hash(&two[0], &two[1])).collect();
        at /= 2;
    }
    branch
}

/// The root that `leaf` at `position` reaches through `branch`.
pub fn root_of_branch(leaf: Hash, position: u32, branch: &[Hash]) -> Hash {
    let climb = |(node, at): (Hash, u32), beside: &Hash| match at % 2 {
        0 => (pair_hash(&node, beside), at / 2),
        _ => (pair_hash(beside, &node), at / 2),
    };
    branch.iter().fold((leaf, position), climb).0
}

/// A coin of some address, with what ties it to the header at `height`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoinProof {
    pub height: u64,
    pub tx: Tx,
    pub output: u32,
    pub position: u32,
    pub branch: Vec<Hash>,
}

impl CoinProof {
    /// `None` when `block` does not hold `point`.
    pub fn of(block: &Block, height: u64, point: &OutPoint) -> Option<Self> {
        let position = block.txs.iter().position(|tx| tx.id() == point.txid)?;
        Some(Self {
            height,
            tx: block.txs[position].clone(),
            output: point.index,
            position: position as u32,
            branch: merkle_branch(&block.txs, position),
        })
    }

    pub fn point(&self) -> OutPoint {
        OutPoint {
            txid: self.tx.id(),
            index: self.output,
        }
    }

    /// The coin, if this proof really ties it to `header` and it really belongs to `address`.
    pub fn check(&self, header: &BlockHeader, address: &Address) -> Option<(OutPoint, Unspent)> {
        let long = self.branch.len() > LONGEST_BRANCH;
        if long || self.tx.encode().len() == INNER_NODE_BYTES {
            return None;
        }
        // A position past the end of the branch would name a leaf no tree of this size has.
        if u64::from(self.position) >> self.branch.len() != 0 {
            return None;
        }
        let leaf = self.tx.id();
        if root_of_branch(leaf, self.position, &self.branch) != header.merkle_root {
            return None;
        }
        let output = self.tx.outputs.get(self.output as usize)?;
        // A data output may hold any bytes, an address too, and pays nobody.
        if output.is_data() || output.lock.as_slice() != address.as_slice() {
            return None;
        }
        let coin = Unspent {
            output: output.clone(),
            height: self.height,
            coinbase: self.tx.is_coinbase(),
        };
        Some((self.point(), coin))
    }
}

/// Where a wallet finds its coins: the unspent set, or coins proven to a node without one.
pub trait Coins {
    fn height(&self) -> u64;
    fn unspent_of(&self, address: &Address) -> Vec<(OutPoint, Unspent)>;
    fn is_mature(&self, entry: &Unspent, spend_height: u64) -> bool;

    fn balance(&self, address: &Address) -> u64 {
        let coins = self.unspent_of(address);
        coins.iter().map(|(_, coin)| coin.output.value).fold(0, u64::saturating_add)
    }
}

impl Coins for Chain {
    fn height(&self) -> u64 {
        Chain::height(self)
    }

    fn unspent_of(&self, address: &Address) -> Vec<(OutPoint, Unspent)> {
        Chain::unspent_of(self, address)
    }

    fn is_mature(&self, entry: &Unspent, spend_height: u64) -> bool {
        Chain::is_mature(self, entry, spend_height)
    }

    fn balance(&self, address: &Address) -> u64 {
        Chain::balance(self, address)
    }
}
