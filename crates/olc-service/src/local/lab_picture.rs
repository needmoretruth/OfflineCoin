//! What the Lab draws: two chains as rows of blocks, read from the miners' own status lines.
//! Nothing here is made up: every block is one a miner holds, with the address its reward went to.
use super::supervisor::{ChainBlock, LocalView};
use olc_lab::Step;

/// Whose a block or a miner is, which decides its color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// The attacker and its helper, or the left group of a split.
    Strong,
    /// The victim, or the right group of a split.
    Other,
    /// A block from before the lab knew the miner.
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mark {
    pub height: u64,
    /// The first hex digits of the block's id.
    pub id: String,
    pub side: Side,
    /// Holds the payment to the victim.
    pub payment: bool,
    /// Holds the attacker's spend of the same coins to itself.
    pub spend: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    pub blocks: Vec<Mark>,
    pub height: u64,
    /// Hashes a second of the miners on this side, measured.
    pub rate: u64,
    /// Of every miner in the lab, 0 to 1.
    pub share: f32,
    /// Cut off from the other side right now.
    pub apart: bool,
}

/// The upper row is the public chain (or the left group), the lower the hidden chain (or the
/// right group). `replaced` holds blocks that were on a chain and are not any more.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Picture {
    pub upper: Row,
    pub lower: Row,
    pub replaced: Vec<Mark>,
}

/// One step and the chains as they stood when it happened.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub step: Step,
    pub picture: Picture,
}

/// The miners of one side, by the addresses their rewards go to.
#[derive(Clone, Debug, Default)]
pub struct Sides {
    pub strong: Vec<String>,
    pub other: Vec<String>,
    pub payment: Option<String>,
    pub spend: Option<String>,
}

impl Sides {
    fn side_of(&self, miner: &str) -> Side {
        if self.strong.iter().any(|address| address == miner) {
            Side::Strong
        } else if self.other.iter().any(|address| address == miner) {
            Side::Other
        } else {
            Side::Unknown
        }
    }

    fn mark(&self, block: &ChainBlock) -> Mark {
        let holds = |id: &Option<String>| id.as_ref().is_some_and(|id| block.txs.contains(id));
        Mark {
            height: block.height,
            id: block.id.chars().take(6).collect(),
            side: self.side_of(&block.miner),
            payment: holds(&self.payment),
            spend: holds(&self.spend),
        }
    }

    /// A row from the chain of `view`, with the rate of every miner on that side.
    pub fn row(&self, view: Option<&LocalView>, side: &[&LocalView], total: u64) -> Row {
        let status = view.and_then(|view| view.status.as_ref());
        let rate: u64 = side.iter().filter_map(|view| view.status.as_ref()).map(|status| status.hashrate).sum();
        Row {
            blocks: status.map(|status| status.chain.iter().map(|block| self.mark(block)).collect()).unwrap_or_default(),
            height: status.map_or(0, |status| status.height),
            rate,
            share: if total == 0 { 0.0 } else { rate as f32 / total as f32 },
            apart: status.is_some_and(|status| status.offline),
        }
    }
}

/// The blocks of `before` that `now` no longer holds at their height.
pub fn replaced_between(before: &Row, now: &Row) -> Vec<Mark> {
    let lowest = now.blocks.first().map_or(0, |mark| mark.height);
    before
        .blocks
        .iter()
        .filter(|old| old.height >= lowest)
        .filter(|old| !now.blocks.iter().any(|new| new.height == old.height && new.id == old.id))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mark(height: u64, id: &str) -> Mark {
        Mark { height, id: id.into(), side: Side::Other, payment: false, spend: false }
    }

    #[test]
    fn blocks_no_longer_on_the_chain_are_the_replaced_ones() {
        let before = Row { blocks: vec![mark(4, "aa"), mark(5, "bb"), mark(6, "cc")], ..Row::default() };
        let now = Row { blocks: vec![mark(4, "aa"), mark(5, "dd"), mark(6, "ee"), mark(7, "ff")], ..Row::default() };
        let gone: Vec<_> = replaced_between(&before, &now).into_iter().map(|mark| mark.id).collect();
        assert_eq!(gone, ["bb", "cc"]);
    }

    #[test]
    fn a_block_is_marked_by_whose_reward_it_holds() {
        let sides = Sides { strong: vec!["olcA".into()], other: vec!["olcV".into()], payment: Some("p1".into()), spend: None };
        let block = ChainBlock { height: 3, id: "abcdef99".into(), miner: "olcA".into(), txs: vec!["p1".into()] };
        let marked = sides.mark(&block);
        assert_eq!((marked.side, marked.payment, marked.spend, marked.id.as_str()), (Side::Strong, true, false, "abcdef"));
    }
}
