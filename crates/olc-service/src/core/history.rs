//! The wallet's recent activity, read from the chain so it survives a restart. It remembers which
//! outputs are the wallet's as it walks, because a payment shows only that it spends them.
use super::explorer::{text_of, NoteRow};
use olc_core::{Address, Block, Chain, Hash, OutPoint};
use std::collections::{BTreeSet, VecDeque};

const SHOWN: usize = 100;
const NOTES_SHOWN: usize = 100;
/// Blocks read in one go, so a long chain never holds the node up.
const STEP: usize = 512;

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
    last: Hash,
    next_height: u64,
    mine: BTreeSet<OutPoint>,
    items: VecDeque<Activity>,
    notes: VecDeque<NoteRow>,
    blocks_mined: u64,
}

impl History {
    pub fn items(&self) -> impl Iterator<Item = &Activity> {
        self.items.iter().rev()
    }

    pub fn blocks_mined(&self) -> u64 {
        self.blocks_mined
    }

    pub fn notes(&self) -> impl Iterator<Item = &NoteRow> {
        self.notes.iter().rev()
    }

    /// Reads some of the blocks since the last call, or starts over after a branch change.
    /// Blocks this node dropped are skipped: what they did is gone from here.
    pub fn follow(&mut self, chain: &Chain, me: &[Address]) {
        if chain.tip() == self.tip {
            return;
        }
        let parted = self.next_height > 0 && chain.id_at(self.next_height - 1) != Some(self.last);
        if parted {
            *self = Self::default();
        }
        self.next_height = self.next_height.max(chain.oldest_held());
        let blocks = chain.blocks_from(self.next_height, STEP);
        for block in &blocks {
            self.read(block, me);
            self.last = block.id();
        }
        if self.next_height > chain.height() {
            self.tip = chain.tip();
        }
    }

    fn read(&mut self, block: &Block, me: &[Address]) {
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
                if output.is_data() {
                    if self.notes.len() >= NOTES_SHOWN {
                        self.notes.pop_front();
                    }
                    self.notes.push_back(NoteRow {
                        height,
                        time_ms: block.header.time_ms,
                        text: text_of(&output.lock),
                    });
                } else if me.iter().any(|own| output.lock.as_slice() == own.as_slice()) {
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
