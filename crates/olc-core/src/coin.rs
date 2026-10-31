//! A coin is a genesis block plus rules.
//!
//! The genesis decides which chain a node is on. The rules decide which blocks a node accepts.
//! As in Bitcoin they are separate: edit the rules on some nodes and those nodes fork away from
//! the others on the same chain history, and no new coin is needed.
use crate::types::{merkle_root, Block, BlockHeader, Tx, TxOut};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use olc_crypto::Address;
use serde::{Deserialize, Serialize};

/// One coin is 100,000,000 base units, as in Bitcoin.
pub const COIN: u64 = 100_000_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rules {
    /// Target time between blocks.
    pub block_time_ms: u64,
    /// Blocks between difficulty changes. 0 keeps the difficulty fixed.
    pub retarget_interval: u64,
    /// Reward of the first block, in base units.
    pub reward: u64,
    /// Blocks between reward halvings. 0 never halves.
    pub halving_interval: u64,
    /// Blocks before a coinbase output can be spent.
    pub coinbase_maturity: u64,
    pub max_block_bytes: u64,
    /// Lowest fee a transaction may pay, in base units.
    pub min_fee: u64,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            block_time_ms: 30_000,
            retarget_interval: 10,
            reward: 50 * COIN,
            halving_interval: 210_000,
            coinbase_maturity: 3,
            max_block_bytes: 4_096,
            min_fee: 0,
        }
    }
}

impl Rules {
    pub fn reward_at(&self, height: u64) -> u64 {
        let halvings = match self.halving_interval {
            0 => 0,
            interval => height / interval,
        };
        if halvings >= 64 {
            0
        } else {
            self.reward >> halvings
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoinConfig {
    pub name: String,
    pub ticker: String,
    /// Written into the genesis block. Change it to start a separate chain.
    pub message: String,
    pub genesis_time_ms: u64,
    pub initial_difficulty: u64,
    /// Coins that exist from the start, as (address, base units).
    pub premine: Vec<(Address, u64)>,
    pub rules: Rules,
}

impl CoinConfig {
    /// The default coin. Two ESP32-S3 boards at 15,000 hashes per second each find a block
    /// about every 30 seconds, and the retarget follows if more hash power joins.
    pub fn olc() -> Self {
        Self {
            name: "OfflineCoin".into(),
            ticker: "OLC".into(),
            message: "OLCC genesis".into(),
            genesis_time_ms: 1_793_491_200_000,
            initial_difficulty: 900_000,
            premine: Vec::new(),
            rules: Rules::default(),
        }
    }

    pub fn genesis(&self) -> Block {
        let outputs: Vec<TxOut> = self
            .premine
            .iter()
            .map(|(address, value)| TxOut {
                value: *value,
                language: 0,
                lock: address.to_vec(),
            })
            .collect();
        let tx = Tx {
            version: 1,
            inputs: Vec::new(),
            outputs,
            coinbase_height: Some(0),
        };
        let txs = vec![tx];
        // The name and message are hashed into the root, so they define the chain.
        let mut root_input = merkle_root(&txs).to_vec();
        root_input.extend_from_slice(self.name.as_bytes());
        root_input.extend_from_slice(self.message.as_bytes());
        let header = BlockHeader {
            version: 1,
            previous: [0; 32],
            merkle_root: olc_crypto::hash(&root_input),
            time_ms: self.genesis_time_ms,
            difficulty: self.initial_difficulty,
            nonce: 0,
        };
        Block { header, txs }
    }
}
