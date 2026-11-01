//! A mining pool, outside the coin: it hands nodes a block paying the pool, at a lower difficulty
//! than the chain asks, counts the real shares that come back, and pays each round by share with
//! an ordinary transaction. The coin has no idea any of this is happening.
use olc_core::{Address, Block, BlockHeader, Hash};
use olc_node::Engine;
use std::collections::{BTreeMap, BTreeSet};

/// What a node needs to work for the pool.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    pub header: BlockHeader,
    pub difficulty: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// The job is gone: the chain moved on, or it was never handed out.
    Stale,
    /// It is not this worker's job, or the header was changed.
    Forged,
    /// The hash does not reach the share difficulty.
    Weak,
    Duplicate,
    Share,
    /// A share that also reached the chain's own difficulty.
    Block {
        height: u64,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub shares: u64,
    pub rejected: u64,
    pub paid: u64,
}

struct Handed {
    worker: Address,
    block: Block,
}

/// A block found, waiting to mature before its reward is shared out.
struct Found {
    height: u64,
    id: Hash,
    value: u64,
    round: BTreeMap<Address, u64>,
}

pub struct Pool {
    payout: Address,
    share_difficulty: u64,
    tip: Hash,
    /// Jobs by merkle root, which the worker's name in the coinbase makes unique.
    jobs: BTreeMap<Hash, Handed>,
    seen: BTreeSet<(Hash, u32)>,
    round: BTreeMap<Address, u64>,
    found: Vec<Found>,
    tally: BTreeMap<Address, Tally>,
    blocks: u64,
    lost: u64,
}

impl Pool {
    /// `payout` is the pool's own address, which must be a key of the node it runs on.
    pub fn new(payout: Address, share_difficulty: u64) -> Self {
        Self {
            payout,
            share_difficulty,
            tip: [0; 32],
            jobs: BTreeMap::new(),
            seen: BTreeSet::new(),
            round: BTreeMap::new(),
            found: Vec::new(),
            tally: BTreeMap::new(),
            blocks: 0,
            lost: 0,
        }
    }

    pub fn tally(&self) -> impl Iterator<Item = (&Address, &Tally)> {
        self.tally.iter()
    }

    pub fn blocks_found(&self) -> u64 {
        self.blocks
    }

    /// Blocks the pool found that left the best chain, so nobody gets paid for them.
    pub fn blocks_lost(&self) -> u64 {
        self.lost
    }

    pub fn share_difficulty(&self) -> u64 {
        self.share_difficulty
    }

    /// True once the chain has moved, which makes every job handed out worthless.
    pub fn is_out_of_date(&mut self, engine: &Engine) -> bool {
        let tip = engine.chain().tip();
        if tip == self.tip {
            return false;
        }
        self.tip = tip;
        self.jobs.clear();
        self.seen.clear();
        true
    }

    /// A block paying the pool. The worker's name in its coinbase gives everyone their own nonces.
    pub fn job_for(&mut self, engine: &Engine, worker: Address, time_ms: u64) -> Job {
        let block = engine.work_template(&self.payout, time_ms, &worker);
        let header = block.header;
        let difficulty = self.share_difficulty.min(header.difficulty);
        self.tally.entry(worker).or_default();
        self.jobs.insert(header.merkle_root, Handed { worker, block });
        Job { header, difficulty }
    }

    fn check(&self, engine: &Engine, worker: &Address, header: &BlockHeader) -> Result<(), Verdict> {
        let handed = self.jobs.get(&header.merkle_root).ok_or(Verdict::Stale)?;
        let mut same = *header;
        same.nonce = handed.block.header.nonce;
        if handed.worker != *worker || same != handed.block.header {
            return Err(Verdict::Forged);
        }
        let difficulty = self.share_difficulty.min(header.difficulty);
        if !engine.work_is_done(header, difficulty) {
            return Err(Verdict::Weak);
        }
        if self.seen.contains(&(header.merkle_root, header.nonce)) {
            return Err(Verdict::Duplicate);
        }
        Ok(())
    }

    /// Checks a share by hashing it. A share that also seals the block goes to the chain.
    pub fn submit(&mut self, engine: &mut Engine, worker: Address, header: &BlockHeader) -> Verdict {
        if let Err(verdict) = self.check(engine, &worker, header) {
            self.tally.entry(worker).or_default().rejected += 1;
            return verdict;
        }
        self.seen.insert((header.merkle_root, header.nonce));
        self.tally.entry(worker).or_default().shares += 1;
        *self.round.entry(worker).or_default() += 1;
        if !engine.work_is_done(header, header.difficulty) {
            return Verdict::Share;
        }
        let Some(handed) = self.jobs.get(&header.merkle_root) else {
            return Verdict::Share;
        };
        let mut block = handed.block.clone();
        block.header.nonce = header.nonce;
        let id = block.id();
        let value = block.txs[0].outputs.iter().map(|output| output.value).sum();
        let height = engine.chain().height() + 1;
        if !engine.submit_block(block) {
            return Verdict::Share;
        }
        self.blocks += 1;
        self.found.push(Found {
            height,
            id,
            value,
            round: std::mem::take(&mut self.round),
        });
        Verdict::Block { height }
    }

    /// Pays every round whose reward has matured. Returns the payment ids.
    pub fn pay(&mut self, engine: &mut Engine) -> Vec<Hash> {
        let chain = engine.chain();
        let maturity = chain.rules().coinbase_maturity;
        let height = chain.height();
        let ripe = |found: &Found| found.height + maturity <= height;
        let (due, waiting): (Vec<Found>, Vec<Found>) =
            std::mem::take(&mut self.found).into_iter().partition(ripe);
        self.found = waiting;
        let mut paid = Vec::new();
        for found in due {
            if engine.chain().id_at(found.height) != Some(found.id) {
                self.lost += 1;
                continue;
            }
            match self.pay_round(engine, &found) {
                Some(id) => paid.push(id),
                // Coins still tied up in an earlier payment: try again at the next block.
                None => self.found.push(found),
            }
        }
        paid
    }

    fn pay_round(&mut self, engine: &mut Engine, found: &Found) -> Option<Hash> {
        let total: u64 = found.round.values().sum();
        let share = |count: u64| u128::from(found.value) * u128::from(count) / u128::from(total);
        let payments: Vec<(Address, u64)> = found
            .round
            .iter()
            .map(|(worker, count)| (*worker, share(*count) as u64))
            .filter(|(_, amount)| *amount > 0)
            .collect();
        let fee = engine.chain().rules().min_fee;
        let spend: u64 = payments.iter().map(|(_, amount)| amount).sum();
        // Whatever the fee takes comes out of the pool's rounding, or waits for coins that cover it.
        let id = engine.pay_many(&payments, fee).ok().filter(|_| spend > 0)?;
        for (worker, amount) in payments {
            self.tally.entry(worker).or_default().paid += amount;
        }
        Some(id)
    }
}
