//! A mining pool, outside the coin, spoken to in Stratum v1 as Bitcoin's pools are. It hands
//! miners a block paying the pool at a lower difficulty than the chain asks, checks each share by
//! hashing it, and pays each block's coins out with one ordinary payment once they can be spent.
//! The coin has no idea any of this is happening.
//!
//! [`Pool`] holds the state; [`session`] reads a connection's lines; [`server::Server`] is the
//! port; [`client::Client`] joins a pool, any Stratum v1 pool whose blocks are OLC's.
pub mod client;
mod found;
pub mod ledger;
mod owed;
mod pool;
pub mod server;
pub mod session;
pub mod stratum;
pub mod vardiff;

pub use client::{Client, ClientView, Link, Work};
pub use ledger::{Payout, Split, MOST_FEE_BPS, MOST_LAST_SHARES};
pub use owed::{pay_owed, Owed};
pub use pool::{meets, BlockState, BlockView, FoundTotal, Plan, Pool, Submitted, WorkerView, REBUILD_MS};
pub use server::{unix_ms, Server, Shared, PORT};
pub use session::{worker_address, Reply};
pub use stratum::Refusal;
