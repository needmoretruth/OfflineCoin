//! The bug exploiter's tricks, in the shape `trouble.rs` gives them: real invalid blocks and
//! payments sent to a node over the network, and that node's own refusal and the rule it named.
//! Another unit builds `trouble.rs`; until it lands this stand-in says every trick is not ready,
//! and the Lab says so instead of making a refusal up.
use olc_core::CoinConfig;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Trick {
    BadSignature,
    DoubleSpend,
    BigReward,
    WeakWork,
    MissingCoin,
}

impl Trick {
    pub const ALL: [Trick; 5] = [Trick::BadSignature, Trick::DoubleSpend, Trick::BigReward, Trick::WeakWork, Trick::MissingCoin];

    /// The key of the trick's short name.
    pub fn key(self) -> &'static str {
        match self {
            Trick::BadSignature => "net.trick_signature",
            Trick::DoubleSpend => "net.trick_double",
            Trick::BigReward => "net.trick_reward",
            Trick::WeakWork => "net.trick_work",
            Trick::MissingCoin => "net.trick_missing",
        }
    }
}

/// What the node did with a trick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub trick: Trick,
    pub refused: bool,
    /// The rule the node named, in its own words.
    pub reason: String,
    /// The node that answered, as it named itself.
    pub by: String,
}

/// The stand-in: no trick is sent yet.
pub fn try_trick(trick: Trick, coin: &CoinConfig, target: &str, funds: &Path) -> Result<Outcome, String> {
    let _ = (trick, coin, target, funds);
    Err(NOT_YET.into())
}

/// What the stand-in answers, so the Lab can say the tricks are not ready yet.
pub const NOT_YET: &str = "not yet";
