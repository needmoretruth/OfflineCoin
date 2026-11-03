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
    /// Coins moved between this wallet's own addresses, as when small coins are merged.
    ToSelf,
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
            self.read(block, me, chain.config().bitcoin_header);
            self.last = block.id();
        }
        if self.next_height > chain.height() {
            self.tip = chain.tip();
        }
    }

    /// On a coin with Bitcoin's header the coinbase's extranonce is a miner's counter, not a note.
    fn read(&mut self, block: &Block, me: &[Address], bitcoin_header: bool) {
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
                if output.is_data() && bitcoin_header && olc_core::is_extranonce(tx, index) {
                    continue;
                }
                if output.is_data() {
                    if self.notes.len() >= NOTES_SHOWN {
                        self.notes.pop_front();
                    }
                    self.notes.push_back(NoteRow {
                        height,
                        time_ms: block.header.time_ms,
                        text: text_of(&output.lock),
                    });
                } else if me.iter().any(|own| output.pays(own)) {
                    to_me += output.value;
                    self.mine.insert(OutPoint {
                        txid,
                        index: index as u32,
                    });
                } else {
                    to_others += output.value;
                }
            }
            let spent = (tx.is_coinbase(), spends_mine);
            let Some((kind, amount)) = kind_of(spent, (to_me, to_others), height) else {
                continue;
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

/// What a transaction did to this wallet, from whether it is a coinbase, whether it spends the
/// wallet's coins and where its outputs go. A genesis coinbase is received, since nobody mined it.
fn kind_of(spent: (bool, bool), (to_me, to_others): (u64, u64), height: u64) -> Option<(ActivityKind, u64)> {
    match spent {
        (true, _) if to_me > 0 && height > 0 => Some((ActivityKind::Mined, to_me)),
        (true, _) if to_me > 0 => Some((ActivityKind::Received, to_me)),
        (false, true) if to_others == 0 => Some((ActivityKind::ToSelf, to_me)),
        (false, true) => Some((ActivityKind::Sent, to_others)),
        (false, false) if to_me > 0 => Some((ActivityKind::Received, to_me)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coins_kept_in_the_wallet_are_not_sent() {
        assert_eq!(kind_of((false, true), (90, 0), 5), Some((ActivityKind::ToSelf, 90)));
        assert_eq!(kind_of((false, true), (40, 50), 5), Some((ActivityKind::Sent, 50)));
        assert_eq!(kind_of((false, false), (50, 0), 5), Some((ActivityKind::Received, 50)));
        assert_eq!(kind_of((true, false), (50, 0), 5), Some((ActivityKind::Mined, 50)));
        assert_eq!(kind_of((false, false), (0, 50), 5), None);
    }

    /// A block's notes come to the list; the miner's counter in its coinbase does not.
    #[test]
    fn the_extranonce_is_no_note() {
        let plugins = olc_core::Plugins { consensus: Box::new(olc_pow::ProofOfWork), languages: olc_lang::all() };
        let chain = Chain::new(olc_core::CoinConfig::olc(), plugins);
        let time_ms = chain.tip_header().time_ms + 1_000;
        let block = chain.block_template_with(&[3; 20], &[], time_ms, b"hello");
        assert!(olc_core::is_extranonce(&block.txs[0], block.txs[0].outputs.len() - 1));
        let mut history = History::default();
        history.read(&block, &[], true);
        let notes: Vec<&str> = history.notes().map(|note| note.text.as_str()).collect();
        assert_eq!(notes, ["hello"]);
        assert_eq!(text_of(&[0; 8]), "0000000000000000");
        assert_eq!(text_of(b"two\nlines"), "two\nlines");
    }
}
