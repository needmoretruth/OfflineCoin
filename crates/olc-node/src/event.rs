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
    /// A payment to this node's wallet is in a block.
    PaymentReceived {
        amount: u64,
    },
    /// This node's wallet sent a payment.
    PaymentSent {
        amount: u64,
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
    /// A peer has more work, but the blocks this node needs from it are gone.
    NeedsHistory {
        peer: PeerId,
    },
    /// A peer's branch parts below the blocks held here, so this node stays on its own.
    ReorgRefused,
    /// This chain now starts from a peer's unspent set. Everything before it rests on trust.
    SnapshotAdopted {
        peer: PeerId,
        height: u64,
    },
    /// The peer's snapshot did not pass.
    SnapshotRefused {
        peer: PeerId,
    },
    /// A data output was over this node's size limit.
    DataRefused,
    /// Small coins went into one with an ordinary transaction.
    CoinsMerged {
        inputs: usize,
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
    /// The peer ahead of this node that no longer holds the blocks it needs, and the peer's height.
    pub needs_history: Option<(PeerId, u64)>,
    /// The peer this chain's unspent set came from, when it did not come from blocks.
    pub assumed_from: Option<u32>,
    /// How far a snapshot download has come, as (parts, all parts).
    pub fetching: Option<(u32, u32)>,
}
