//! What nodes say to each other. There is one sync mechanism: a node announces its tip, and a
//! peer that wants it asks for blocks backward until it reaches one it knows.
use alloc::vec::Vec;
use olc_core::{Block, Hash, Tx};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Message {
    Tip {
        genesis: Hash,
        tip: Hash,
        height: u64,
        work: u128,
    },
    GetBlock(Hash),
    Block(Block),
    Tx(Tx),
}

impl Message {
    pub fn encode(&self) -> Vec<u8> {
        postcard::to_allocvec(self).unwrap_or_default()
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        postcard::from_bytes(bytes).ok()
    }
}
