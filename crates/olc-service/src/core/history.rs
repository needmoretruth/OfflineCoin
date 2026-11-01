//! The wallet's recent activity, read from the chain so it survives a restart. It remembers which
//! outputs are the wallet's as it walks, because a payment shows only that it spends them.
use olc_core::{Address, Block, Chain, Hash, OutPoint};
use std::collections::{BTreeSet, VecDeque};

const SHOWN: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivityKind {
    Mined,
    Received,
    Sent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Activity {
    pub kind: ActivityKind,
    /// `None` while the payment waits in the pool.
    pub height: Option<u64>,
    pub amount: u64,
    pub time_ms: u64,
    pub id: Hash,
}

#[derive(Default)]
pub struct History {
    tip: Hash,
    next_height: u64,
    mine: BTreeSet<OutPoint>,
    items: VecDeque<Activity>,
    blocks_mined: u64,
}

impl History {
    pub fn items(&self) -> impl Iterator<Item = &Activity> {
        self.items.iter().rev()
    }

    pub fn blocks_mined(&self) -> u64 {
        self.blocks_mined
    }

    /// Reads the blocks since the last call, or all of them after a branch change.
    pub fn follow(&mut self, chain: &Chain, me: &Address) {
        if chain.tip() == self.tip {
            return;
        }
        let blocks = chain.best_chain_from(self.next_height);
        let joins = self.next_height == 0
            || blocks
                .first()
                .is_some_and(|block| block.header.previous == self.tip);
        if joins {
            blocks.iter().for_each(|block| self.read(block, me));
        } else {
            *self = Self::default();
            let all = chain.best_chain_from(0);
            all.iter().for_each(|block| self.read(block, me));
        }
        self.tip = chain.tip();
        self.next_height = chain.height() + 1;
    }

    fn read(&mut self, block: &Block, me: &Address) {
        let height = self.next_height;
        self.next_height += 1;
        for tx in &block.txs {
            let txid = tx.id();
            let spends_mine = tx
                .inputs
                .iter()
                .filter(|input| self.mine.remove(&input.previous))
                .count()
                > 0;
            let (mut to_me, mut to_others) = (0, 0);
            for (index, output) in tx.outputs.iter().enumerate() {
                if output.lock.as_slice() == me.as_slice() {
                    to_me += output.value;
                    self.mine.insert(OutPoint {
                        txid,
                        index: index as u32,
                    });
                } else {
                    to_others += output.value;
                }
            }
            let (kind, amount) = match (tx.is_coinbase(), spends_mine) {
                (true, _) if to_me > 0 && height > 0 => (ActivityKind::Mined, to_me),
                (true, _) if to_me > 0 => (ActivityKind::Received, to_me),
                (false, true) => (ActivityKind::Sent, to_others),
                (false, false) if to_me > 0 => (ActivityKind::Received, to_me),
                _ => continue,
            };
            if kind == ActivityKind::Mined {
                self.blocks_mined += 1;
            }
            if self.items.len() >= SHOWN {
                self.items.pop_front();
            }
            self.items.push_back(Activity {
                kind,
                height: Some(height),
                amount,
                time_ms: block.header.time_ms,
                id: txid,
            });
        }
    }
}
