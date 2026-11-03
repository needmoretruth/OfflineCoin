//! Payment channels on OLC, after Lightning (BOLT 2, 3, 7 and 11) on OLC's own messages. Two
//! nodes lock coins in a two-of-two output, pay each other by signing new states off the chain,
//! pay others through channels in between with hash locks, and close together or alone. A side
//! that takes an old state to the chain loses the channel's coins to the other's penalty.
//!
//! The pieces, bottom up: [`keys`] and [`script`] are BOLT 3's keys and locks in gamma, [`tx`] its
//! transactions; [`channel`] and [`opening`] are one channel's steps; [`node`] holds every channel
//! of a node with its files, payments and the watcher that sends penalties.
pub mod bytes;
pub mod channel;
mod flow;
mod gossip;
mod orders;
pub mod graph;
pub mod invoice;
pub mod keys;
pub mod node;
pub mod opening;
pub mod script;
pub mod store;
pub mod tx;
pub mod watch;
pub mod wire;

pub use bytes::{Bytes, Key};
pub use channel::{Channel, Phase};
pub use invoice::Invoice;
pub use node::{ChannelNode, Ground, Out, PayState, Said};
pub use orders::{ChannelView, InvoiceView, PaymentView, Quote, Refusal};
pub use wire::{ChannelId, NodeId};
