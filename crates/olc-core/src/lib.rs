//! The chain: types, rules and validation. Consensus and the lock languages are plugged in
//! through the traits in [`plugin`], so this crate never changes when one of them does.
#![no_std]
extern crate alloc;

pub mod chain;
pub mod coin;
mod ledger;
mod locator;
mod orphans;
pub mod plugin;
pub mod types;

pub use chain::{AddOutcome, Chain, ChainError};
pub use coin::{CoinConfig, Rules};
pub use olc_crypto::{Address, Hash};
pub use plugin::{Consensus, LockLanguage, Plugins, RetargetInput, SpendContext};
pub use types::{Block, BlockHeader, OutPoint, Tx, TxIn, TxOut};
