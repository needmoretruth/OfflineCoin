//! What an [`Engine`](crate::Engine) says about itself and to the network.
use crate::engine::PeerId;
use crate::message::Message;
use olc_core::{ChainError, Hash};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outgoing {
    To(PeerId, Message),
    All {
        except: Option<PeerId>,
        message: Message,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Mined {
        height: u64,
    },
    BlockAccepted {
        height: u64,
    },
    Reorganized {
        replaced: u64,
        height: u64,
    },
    BlockRejected {
        reason: ChainError,
    },
    TxAccepted,
    TxRejected {
        reason: ChainError,
    },
    /// A payment from a block that left the best chain is waiting to be mined again.
    TxReturned {
        id: Hash,
    },
    /// A payment was thrown out after a reorganisation. No reason means the pool was full.
    TxDropped {
        id: Hash,
        reason: Option<ChainError>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status {
    pub height: u64,
    pub tip: Hash,
    pub work: u128,
    pub difficulty: u64,
    pub balance: u64,
    pub pool: usize,
    pub peers: usize,
    /// The height being caught up to, while well behind it.
    pub syncing_to: Option<u64>,
}
