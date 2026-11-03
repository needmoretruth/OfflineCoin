//! What an experiment tells a watcher as it happens, so a window can draw each step beside its
//! words. The words go to `Say` as before; these carry only what a picture needs.
use std::sync::Arc;

/// One thing that just happened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// Hashes a second on each side, measured.
    Power { attacker: u64, victim: u64 },
    /// Both chains end in the same block.
    LinedUp { height: u64 },
    /// The attacker paid the victim, and the victim holds the payment.
    Paid { payment: String },
    /// The attacker mines out of sight from here.
    Dark,
    /// The payment is this deep on the public chain.
    Confirmed { depth: u64, of: u64 },
    /// The attacker spent the same coins on itself on its hidden chain.
    SpentAgain { spend: String },
    /// The hidden chain has more work and holds that spend.
    Heavier,
    /// The attacker stops racing this far behind.
    GaveUp { behind: u64 },
    /// The hidden chain is shown to everyone.
    Revealed,
    /// Blocks were replaced from `height`. `hidden_won` says whose: the public chain's when the
    /// hidden chain won, the hidden chain's when it lost.
    Replaced { height: u64, blocks: u64, hidden_won: bool },
    /// The victim threw the payment out: its coins are spent on the winning chain.
    Refused,
    /// The payment is on no chain. The double spend worked.
    Gone,
    /// The payment is still on the public chain. The double spend failed.
    Stays,
    /// The two groups are cut apart.
    Cut { height: u64 },
    /// The two groups are joined again.
    Rejoined,
    /// Every miner is on one chain again. `left_won` says which group's blocks stayed.
    Settled { left_won: bool, height: u64, blocks: u64 },
}

/// How a double spend ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Gone,
    Stays,
}

pub type Watch = Arc<dyn Fn(Step) + Send + Sync>;
