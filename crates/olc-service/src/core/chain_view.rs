//! The latest blocks with who mined each, the branch that raced them and the last one that lost:
//! what a person watches to see their own small network mine. The node's thread keeps this up to
//! date one block at a time, so a new tip costs one block read and a quiet publish costs nothing.
use olc_core::{Block, Chain, Hash, ADDRESS_LANGUAGE};
use olc_wallet::format_address;
use std::collections::VecDeque;

/// How many of the latest blocks the shares count. At 60 blocks a miner with half the speed reads
/// 50 % give or take 6 points, steady enough to tell two miners apart, and a changed speed shows
/// within 60 blocks: six minutes at a six second block.
pub const SHARE_WINDOW: usize = 60;
/// A branch more than this far below the top is history, not a race.
pub const RIVAL_DEPTH: u64 = 6;
/// A move of the best chain this long or longer is a sync, read again from the start.
const REORG_LOOK: u64 = 100;

/// Who found a block, as far as this computer can tell from the address its reward went to.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Miner {
    /// This app's own wallet or the address its node mines to.
    You,
    /// A miner process on this computer, by its name.
    Here(String),
    /// A board, by its name.
    Board(String),
    /// Some other computer, by the address its reward went to.
    Away(String),
    /// The pool this app runs: its blocks pay this app's wallet and carry the pool's tag.
    OwnPool,
    /// Another pool, by the address its blocks pay.
    Pool(String),
    /// A reward with no plain address.
    #[default]
    Unknown,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MinedBlock {
    pub height: u64,
    pub id: Hash,
    pub time_ms: u64,
    /// Payments in it, the reward aside.
    pub payments: usize,
    /// What the reward paid out, fees included.
    pub reward: u64,
    pub difficulty: u64,
    /// The address the reward went to, as people write it.
    pub payee: Option<String>,
    /// The reward carries a pool's tag.
    pub pool: bool,
    pub miner: Miner,
}

impl MinedBlock {
    pub fn of(height: u64, block: &Block) -> Self {
        let coinbase = block.txs.first();
        let outputs = coinbase.map(|tx| tx.outputs.as_slice()).unwrap_or_default();
        let paid = outputs.iter().filter(|output| !output.is_data());
        let payee = outputs
            .iter()
            .filter(|output| output.language == ADDRESS_LANGUAGE)
            .max_by_key(|output| output.value)
            .and_then(|output| <[u8; 20]>::try_from(output.lock.as_slice()).ok());
        Self {
            height,
            id: block.id(),
            time_ms: block.header.time_ms,
            payments: block.txs.len().saturating_sub(1),
            reward: paid.map(|output| output.value).sum(),
            difficulty: block.header.difficulty,
            payee: payee.map(|address| format_address(&address)),
            pool: outputs.iter().any(|output| output.is_data() && output.lock.starts_with(olc_pool::POOL_TAG)),
            miner: Miner::Unknown,
        }
    }

    /// The first eight hex digits of the id, as the explorer shows it.
    pub fn short_hash(&self) -> String {
        self.id[..4].iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

/// A branch beside the best chain, oldest block first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Branch {
    pub blocks: Vec<MinedBlock>,
    /// When this computer saw it.
    pub seen_ms: u64,
}

impl Branch {
    pub fn first(&self) -> u64 {
        self.blocks.first().map_or(0, |block| block.height)
    }

    pub fn last(&self) -> u64 {
        self.blocks.last().map_or(0, |block| block.height)
    }
}

/// What the network's speed was read from: this many blocks, over this long up to now. Miners
/// whose speed this app measures count by that speed; the blocks others found count as their work
/// over the span, so a change of their speed shows in full once the span has passed since it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reading {
    pub blocks: usize,
    pub span_ms: u64,
    /// What those blocks alone say, whoever found them, before anything measured.
    pub rate: u64,
    /// Of those blocks, the ones found by miners whose speed this app does not measure.
    pub others: usize,
    /// Their work over the span: the part of the network's speed that is read, not measured.
    pub others_rate: u64,
}

/// The recent average as Bitcoin reads it from blocks, beside what this computer measured over the
/// same time. Blocks hold only the work that made a block: hashes on a block another miner already
/// found, before the news arrived, and blocks that lost a race leave nothing, so with several
/// miners the blocks read lower than the miners measured, and by luck alone they stray about two
/// parts in √n either way.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Average {
    /// The blocks read: those of the last [`AVERAGE_SPAN_MS`], up to the [`SHARE_WINDOW`] kept.
    pub blocks: usize,
    pub span_ms: u64,
    /// Their work over their time.
    pub rate: u64,
    /// The mean speed this computer measured over the same time, when every one of those blocks
    /// was found by a miner whose speed it measures and it measured all that time.
    pub measured: Option<u64>,
}

/// How far back the recent average reads: the ten minutes the speed line draws. Older blocks may
/// stand across hours nobody mined, and an average over them says nothing about now.
pub const AVERAGE_SPAN_MS: u64 = 600_000;

impl Average {
    /// How far, in whole percent, a speed read from this many blocks strays by luck in about
    /// nineteen readings of twenty.
    pub fn scatter(&self) -> u64 {
        let blocks = u32::try_from(self.blocks).unwrap_or(u32::MAX).max(1);
        (200.0 / f64::from(blocks).sqrt()).ceil() as u64
    }

    /// The average against the measured mean in whole percent: below it when negative.
    pub fn off(&self) -> Option<i64> {
        let measured = i128::from(self.measured.filter(|measured| *measured > 0)?);
        let percent = (i128::from(self.rate) - measured) * 100 / measured;
        i64::try_from(percent).ok()
    }
}

/// One miner's part of the latest blocks.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Share {
    pub miner: Miner,
    pub blocks: u32,
    /// Its speed now, where this computer hears it.
    pub hashrate: Option<u64>,
    /// For a pool: the miners here working in it, whose blocks count as the pool's.
    pub workers: Vec<Miner>,
}

/// Everything the block strip, the race bars, the speed line and the next block bar draw.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChainView {
    /// The latest blocks of the best chain, newest first, at most [`SHARE_WINDOW`].
    pub latest: Vec<MinedBlock>,
    /// A branch this node holds that races the top of the best chain right now.
    pub rival: Option<Branch>,
    /// The last blocks that left the best chain because another branch won.
    pub replaced: Option<Branch>,
    /// Each miner's blocks among `latest`, most first, and miners that hash with none yet.
    pub shares: Vec<Share>,
    /// Hashes a second on this computer, this app and its miners together, oldest first, as
    /// (unix ms, rate) over the last ten minutes.
    pub trail: Vec<(u64, u64)>,
    pub hashrate_here: u64,
    /// The network's speed now: the measured speed of every miner this app knows, and for the
    /// latest blocks others found, their work over their time up to now, as Bitcoin's
    /// getnetworkhashps reads it. Never below `hashrate_here`, since the network holds this
    /// computer. With too few blocks to tell who else mines, the speeds this computer hears.
    pub hashrate_all: u64,
    /// The blocks and the time that speed was read from; `None` while too few blocks are kept.
    pub network: Option<Reading>,
    /// The work of the blocks of the last ten minutes over their time; `None` while too few.
    pub average: Option<Average>,
    /// How long the next block takes at that speed and the current difficulty.
    pub expected_ms: Option<u64>,
    /// How long the last blocks really took, on average, to set beside the expected wait.
    pub average_ms: Option<u64>,
}

impl ChainView {
    /// When the newest block was made, by its own clock.
    pub fn last_block_ms(&self) -> Option<u64> {
        self.latest.first().map(|block| block.time_ms)
    }

    /// How many blocks the shares count.
    pub fn window(&self) -> u32 {
        self.shares.iter().map(|share| share.blocks).sum()
    }

    /// This computer's part of the network's speed in whole percent, and in whole points how far
    /// off it may be. `None` while too few blocks tell who else mines. With every block found by
    /// miners whose speed is measured, the part is exact. Otherwise the others' speed is read
    /// from their k blocks, off by about one part in √k, and moves the part by its share of the
    /// whole as much: twice that holds about nine readings in ten.
    pub fn share(&self) -> Option<(u64, u64)> {
        let reading = self.network?;
        let (here, all) = (u128::from(self.hashrate_here), u128::from(self.hashrate_all));
        let percent = u64::try_from((here * 100).checked_div(all)?).unwrap_or(100).min(100);
        if reading.others == 0 {
            return Some((percent, 0));
        }
        // Speeds stay far below 2^53, where a float holds every whole number.
        let part = |rate: u64| rate as f64 / self.hashrate_all as f64;
        let scatter = f64::from(u32::try_from(reading.others).unwrap_or(u32::MAX)).sqrt();
        let error = 200.0 * part(self.hashrate_here) * part(reading.others_rate) / scatter;
        Some((percent, error.ceil() as u64))
    }
}

/// The node thread's part: blocks as they come, and the branches beside them.
#[derive(Default)]
pub struct ChainWatch {
    /// Oldest first.
    blocks: VecDeque<MinedBlock>,
    tip: Hash,
    replaced: Option<Branch>,
    rival: Option<Branch>,
    /// The tips other miners hold and this node's tip, when the rival was last looked for.
    rivals_for: (Vec<Hash>, Hash),
}

fn on_best(chain: &Chain, id: &Hash) -> Option<u64> {
    let height = chain.height_of_block(id)?;
    (chain.id_at(height) == Some(*id)).then_some(height)
}

fn height_in(block: &Block, chain: &Chain) -> u64 {
    let id = block.id();
    chain
        .height_of_block(&id)
        .or_else(|| block.txs.first().and_then(|tx| tx.coinbase_height))
        .unwrap_or(0)
}

impl ChainWatch {
    /// Follows a new tip: one block read when it extends the last, the whole window again after a
    /// jump, and the losing side kept when the best chain moved to another branch.
    pub fn follow(&mut self, chain: &Chain, now_ms: u64) {
        let tip = chain.tip();
        if tip == self.tip {
            return;
        }
        let old = std::mem::replace(&mut self.tip, tip);
        let height = chain.height();
        match on_best(chain, &old) {
            Some(at) if !self.blocks.is_empty() && at <= height && height - at <= SHARE_WINDOW as u64 => {
                let fresh = chain.blocks_from(at + 1, (height - at) as usize);
                let rows = fresh.iter().enumerate().map(|(index, block)| MinedBlock::of(at + 1 + index as u64, block));
                self.blocks.extend(rows);
            }
            _ => {
                self.keep_replaced(chain, &old, now_ms);
                let first = (height + 1).saturating_sub(SHARE_WINDOW as u64).max(chain.oldest_held()).max(1);
                let blocks = chain.blocks_from(first, SHARE_WINDOW);
                let rows = blocks.iter().enumerate().map(|(index, block)| MinedBlock::of(first + index as u64, block));
                self.blocks = rows.collect();
            }
        }
        while self.blocks.len() > SHARE_WINDOW {
            self.blocks.pop_front();
        }
    }

    fn keep_replaced(&mut self, chain: &Chain, old: &Hash, now_ms: u64) {
        let Some(old_height) = chain.height_of_block(old) else {
            return;
        };
        let still_best = on_best(chain, old).is_some();
        if still_best || *old == [0; 32] || chain.height().abs_diff(old_height) > REORG_LOOK {
            return;
        }
        let (left, _) = chain.branches_from(old);
        if left.is_empty() {
            return;
        }
        let blocks = left.iter().map(|block| MinedBlock::of(height_in(block, chain), block));
        self.replaced = Some(Branch { blocks: blocks.collect(), seen_ms: now_ms });
    }

    /// Looks for a branch near the top among the tips other miners say they hold. Only blocks this
    /// node keeps can be shown, so a tip it never received stays out.
    pub fn look_for_rivals(&mut self, chain: &Chain, tips: &[Hash], now_ms: u64) {
        let key = (tips.to_vec(), chain.tip());
        if key == self.rivals_for {
            return;
        }
        self.rivals_for = key;
        let top = chain.height();
        let mut best: Option<Branch> = None;
        for tip in tips {
            let mut blocks = Vec::new();
            let mut cursor = *tip;
            while blocks.len() < RIVAL_DEPTH as usize && on_best(chain, &cursor).is_none() {
                let Some(block) = chain.block(&cursor) else {
                    break;
                };
                cursor = block.header.previous;
                blocks.push(MinedBlock::of(height_in(&block, chain), &block));
            }
            let joins = on_best(chain, &cursor).is_some();
            let near = blocks.first().is_some_and(|newest| newest.height + RIVAL_DEPTH > top);
            if !joins || !near {
                continue;
            }
            blocks.reverse();
            if best.as_ref().is_none_or(|kept| blocks.len() > kept.blocks.len()) {
                let seen_ms = self.rival.as_ref().filter(|old| old.blocks == blocks).map_or(now_ms, |old| old.seen_ms);
                best = Some(Branch { blocks, seen_ms });
            }
        }
        self.rival = best;
    }

    /// The node's side of the view, newest block first, miners still to be named.
    pub fn view(&self) -> ChainView {
        ChainView {
            latest: self.blocks.iter().rev().cloned().collect(),
            rival: self.rival.clone(),
            replaced: self.replaced.clone(),
            ..ChainView::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use olc_core::{Address, CoinConfig, Consensus, Plugins};
    use olc_pow::ProofOfWork;
    use olc_wallet::Wallet;

    fn chain() -> Chain {
        let mut coin = CoinConfig::olc();
        coin.initial_difficulty = 8;
        coin.rules.retarget_interval = 0;
        Chain::new(coin, Plugins { consensus: Box::new(ProofOfWork), languages: olc_lang::all() })
    }

    fn mine(chain: &mut Chain, miner: &Address) -> Block {
        let time = chain.tip_header().time_ms + 1_000;
        let mut block = chain.block_template(miner, &[], time);
        assert!(ProofOfWork.try_seal(&mut block.header, 1_000_000));
        chain.add_block(block.clone()).unwrap();
        block
    }

    /// Alice's node mines one block, then Bob's longer branch arrives: the strip shows Bob's
    /// blocks and keeps Alice's as the one that lost, and Bob's tip seen early is a race.
    #[test]
    fn blocks_say_who_mined_them_and_a_lost_race_is_kept() {
        let alice = Wallet::from_secret([1; 32]).unwrap().address();
        let bob = Wallet::from_secret([2; 32]).unwrap().address();
        let (mut a, mut b) = (chain(), chain());
        let shared = mine(&mut a, &alice);
        b.add_block(shared).unwrap();
        let mut watch = ChainWatch::default();
        watch.follow(&a, 10);
        let lost = mine(&mut a, &alice);
        watch.follow(&a, 20);
        let view = watch.view();
        assert_eq!(view.latest.len(), 2);
        assert_eq!(view.latest[0].payee.as_deref(), Some(format_address(&alice).as_str()));
        assert_eq!(view.latest[0].reward, a.config().rules.reward_at(2));
        assert_eq!(view.latest[0].short_hash().len(), 8);

        let first = mine(&mut b, &bob);
        a.add_block(first.clone()).unwrap();
        watch.look_for_rivals(&a, &[first.id()], 30);
        let rival = watch.view().rival.expect("Bob's block races Alice's");
        assert_eq!((rival.first(), rival.last()), (2, 2));
        assert_eq!(rival.blocks[0].payee.as_deref(), Some(format_address(&bob).as_str()));

        let second = mine(&mut b, &bob);
        a.add_block(second).unwrap();
        watch.follow(&a, 40);
        watch.look_for_rivals(&a, &[b.tip()], 40);
        let view = watch.view();
        assert_eq!(view.latest.iter().map(|block| block.height).collect::<Vec<_>>(), [3, 2, 1]);
        assert_eq!(view.latest[0].payee.as_deref(), Some(format_address(&bob).as_str()));
        let replaced = view.replaced.expect("Alice's block lost");
        assert_eq!(replaced.blocks.len(), 1);
        assert_eq!((replaced.first(), replaced.blocks[0].id), (2, lost.id()));
        assert!(view.rival.is_none(), "the race is over");
    }
}
