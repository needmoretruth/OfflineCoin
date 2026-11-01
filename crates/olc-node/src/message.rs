//! What nodes say to each other. There is one sync mechanism: a node announces its tip, and a
//! peer behind it asks for the blocks after its own position.
use crate::hello::Hello;
use alloc::vec::Vec;
use olc_core::{Block, Hash, Tx};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Message {
    Tip {
        genesis: Hash,
        tip: Hash,
        /// So a tip built on a refused block is known for what it is without fetching it.
        previous: Hash,
        height: u64,
        work: u128,
    },
    /// Asks for the blocks after the newest of these the peer also has.
    GetBlocks(Vec<(u64, Hash)>),
    Block(Block),
    Tx(Tx),
    /// Running boards from afar. Carried for whoever reads it; the chain ignores it.
    Control(Vec<u8>),
    /// Asks a node what it is, whichever coin it runs.
    Who,
    /// The answer to [`Message::Who`].
    Hello(alloc::boxed::Box<Hello>),
    /// This peer no longer holds the blocks that were asked for. `oldest` is its first one.
    Pruned { oldest: u64 },
    /// Asks for part of a peer's unspent set. Only ever sent because a person chose to.
    GetSnapshot { part: u32 },
    /// One part of a snapshot, about a kilobyte, whatever the transport.
    SnapshotPart {
        height: u64,
        /// Hash of the whole snapshot.
        sum: Hash,
        part: u32,
        parts: u32,
        bytes: Vec<u8>,
    },
}

impl Message {
    pub fn encode(&self) -> Vec<u8> {
        postcard::to_allocvec(self).unwrap_or_default()
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        postcard::from_bytes(bytes).ok()
    }
}
