//! The chain: types, rules and validation. Consensus and the lock languages are plugged in
//! through the traits in [`plugin`], so this crate never changes when one of them does.
#![no_std]
extern crate alloc;

pub mod chain;
pub mod coin;
mod connect;
mod ledger;
mod locator;
mod log;
pub mod medium;
mod orphans;
pub mod plugin;
mod rules;
mod snapshot;
mod state;
mod tree;
pub mod types;

pub use chain::{AddOutcome, Chain, ChainError, Held, FEWEST_RECENT};
pub use coin::{CoinConfig, Rules};
pub use locator::Served;
pub use medium::{Keep, Medium, MemoryMedium};
pub use olc_crypto::{Address, Hash};
pub use plugin::{Consensus, LockLanguage, Plugins, RetargetInput, SpendContext};
pub use snapshot::SnapshotError;
pub use state::Opened;
pub use types::{Block, BlockHeader, OutPoint, Tx, TxIn, TxOut, DATA_LANGUAGE};
