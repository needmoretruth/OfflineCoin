//! The chain: types, rules and validation. Consensus and the lock languages are plugged in
//! through the traits in [`plugin`], so this crate never changes when one of them does.
#![no_std]
extern crate alloc;

pub mod amount;
pub mod bits;
pub mod chain;
pub mod coin;
mod added;
mod connect;
pub mod headers;
mod ledger;
mod locator;
mod log;
pub mod medium;
mod pace;
mod orphans;
pub mod plugin;
pub mod proof;
mod rules;
mod shelf;
pub mod sighash;
pub mod timing;
mod snapshot;
mod state;
mod tree;
mod tx_form;
pub mod types;

pub use chain::template::{ancestors, by_package, coinbase_split, is_extranonce, pays_more, Waiting, EXTRANONCE_BYTES};
pub use chain::{AddOutcome, Chain, ChainError, Held, FEWEST_RECENT};
pub use coin::{CoinConfig, Rules};
pub use timing::DifficultyRule;
pub use headers::{HeaderChain, HeaderRefused, Offered};
pub use ledger::Unspent;
pub use locator::Served;
pub use medium::{Keep, Medium, MemoryMedium};
pub use olc_crypto::{Address, Hash};
pub use plugin::{Consensus, LockLanguage, PerBlockInput, Plugins, RetargetInput, SpendContext, Spending};
pub use proof::{CoinProof, Coins};
pub use shelf::Shelf;
pub use snapshot::SnapshotError;
pub use state::Opened;
pub use types::{Block, BlockHeader, OutPoint, Tx, TxIn, TxOut, ADDRESS_LANGUAGE, DATA_LANGUAGE, SEQUENCE_FINAL};
