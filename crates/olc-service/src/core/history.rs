//! The wallet's activity, read from the chain so it survives a restart. It remembers which
//! outputs are the wallet's as it walks, because a payment shows only that it spends them. What
//! waits in the pool is read again at every look, so a waiting row never outlives its payment.
use super::address_index::AddressIndex;
use super::explorer::{block_time_ms, text_of, Explored, NoteRow, PendingRow};
use olc_core::{Address, Block, Chain, Hash, OutPoint, Tx};
use olc_wallet::parse_address;
use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

/// Rows the history page keeps: some days of a miner's blocks every few seconds, in about a
/// megabyte. Older rows stay in their blocks.
pub const KEPT: usize = 10_000;
/// Rows the window carries beside the whole list, for a short list such as Home's.
pub const RECENT: usize = 100;
const NOTES_SHOWN: usize = 100;
/// Blocks read in one go, so a long chain never holds the node up.
const STEP: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ActivityKind {
    Mined,
    Received,
    Sent,
    /// Coins moved to another address of this wallet, as when old keys move to the words.
    ToSelf,
    /// Several coins of one address joined into one coin of that address, to keep the wallet
    /// small. Only the fee leaves.
    Joined,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Activity {
    pub kind: ActivityKind,
    /// `None` while the payment waits in the pool.
    pub height: Option<u64>,
    pub amount: u64,
    /// When its block was made, or when this node first saw it waiting.
    pub time_ms: u64,
    pub id: Hash,
    /// What it paid to be mined, when every coin it spent is known here.
    pub fee: Option<u64>,
    pub size: u64,
    /// Coins of this wallet it spent: for a join, how many became one.
    pub coins_in: usize,
    /// For a mined row, the first block in which its coins can be spent.
    pub ripe_at: Option<u64>,
}

impl Activity {
    /// A row still waiting for a block, with nothing known yet but its kind and id.
    pub fn waiting(kind: ActivityKind, id: Hash) -> Self {
        Self { kind, height: None, amount: 0, time_ms: 0, id, fee: None, size: 0, coins_in: 0, ripe_at: None }
    }
}

/// One transaction as it touches this wallet, before it is named.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Shape {
    coinbase: bool,
    /// The wallet's coins it spends, each with its address and value.
    spent: Vec<(Address, u64)>,
    inputs: usize,
    /// Outputs to the wallet, by address, and their total.
    to_me: Vec<(Address, u64)>,
    to_others: u64,
    /// Outputs that pay someone or something, data left out.
    outputs: usize,
}

impl Shape {
    fn received(&self) -> u64 {
        self.to_me.iter().map(|(_, value)| value).sum()
    }

    /// The fee, when every coin it spends is the wallet's.
    fn fee(&self) -> Option<u64> {
        let spent: u64 = self.spent.iter().map(|(_, value)| value).sum();
        let all_known = !self.coinbase && self.spent.len() == self.inputs;
        all_known.then(|| spent.saturating_sub(self.received() + self.to_others))
    }

    /// The amount a payment to the wallet's own address moved, its change left out. The wallet
    /// puts the change last, and the change may go to the very address paid, so it is told
    /// apart by place, not by address.
    fn moved(&self) -> u64 {
        match self.to_me.as_slice() {
            [paid @ .., _change] if !paid.is_empty() => paid.iter().map(|(_, value)| value).sum(),
            _ => self.received(),
        }
    }

    /// Several coins of one address, one coin back to that same address.
    fn is_join(&self) -> bool {
        let Some((first, _)) = self.spent.first() else {
            return false;
        };
        let one_address = self.spent.iter().all(|(address, _)| address == first);
        let back = matches!(self.to_me.as_slice(), [(address, _)] if address == first);
        self.spent.len() >= 2 && self.spent.len() == self.inputs && one_address && back && self.outputs == 1
    }
}

/// What a transaction did to this wallet. A genesis coinbase is received, since nobody mined it.
fn kind_of(shape: &Shape, height: u64) -> Option<(ActivityKind, u64)> {
    let (to_me, spends) = (shape.received(), !shape.spent.is_empty());
    match (shape.coinbase, spends) {
        (true, _) if to_me > 0 && height > 0 => Some((ActivityKind::Mined, to_me)),
        (true, _) if to_me > 0 => Some((ActivityKind::Received, to_me)),
        (false, true) if shape.to_others == 0 && shape.is_join() => Some((ActivityKind::Joined, to_me)),
        (false, true) if shape.to_others == 0 => Some((ActivityKind::ToSelf, shape.moved())),
        (false, true) => Some((ActivityKind::Sent, shape.to_others)),
        (false, false) if to_me > 0 => Some((ActivityKind::Received, to_me)),
        _ => None,
    }
}

/// The shape of `tx`, with `owner` naming the wallet's address and value of a coin it spends.
fn shape_of(tx: &Tx, me: &[Address], mut owner: impl FnMut(&OutPoint) -> Option<(Address, u64)>) -> Shape {
    let mut shape = Shape { coinbase: tx.is_coinbase(), inputs: tx.inputs.len(), ..Shape::default() };
    shape.spent = tx.inputs.iter().filter_map(|input| owner(&input.previous)).collect();
    for output in tx.outputs.iter().filter(|output| !output.is_data()) {
        shape.outputs += 1;
        match me.iter().find(|own| output.pays(&own[..])) {
            Some(own) => shape.to_me.push((*own, output.value)),
            None => shape.to_others += output.value,
        }
    }
    shape
}

/// What the pool holds as this node sees it: the wallet's own waiting rows, newest first, and
/// every waiting payment, oldest first.
#[derive(Clone, Debug, Default)]
pub struct PoolSeen {
    pub mine: Vec<Activity>,
    pub all: Vec<PendingRow>,
}

#[derive(Default)]
pub struct History {
    tip: Hash,
    last: Hash,
    next_height: u64,
    mine: BTreeMap<OutPoint, (Address, u64)>,
    items: VecDeque<Activity>,
    /// The same rows newest first, made again only when they change.
    shown: Arc<Vec<Activity>>,
    notes: VecDeque<NoteRow>,
    blocks_mined: u64,
    index: AddressIndex,
    /// When this node first saw each waiting payment.
    first_seen: BTreeMap<Hash, u64>,
}

impl History {
    pub fn items(&self) -> impl Iterator<Item = &Activity> {
        self.items.iter().rev()
    }

    /// Every row kept, newest first. Cheap to clone.
    pub fn all(&self) -> Arc<Vec<Activity>> {
        Arc::clone(&self.shown)
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
            self.read(chain, block, me);
            self.last = block.id();
        }
        if !blocks.is_empty() {
            self.shown = Arc::new(self.items.iter().rev().cloned().collect());
        }
        if self.next_height > chain.height() {
            self.tip = chain.tip();
        }
    }

    fn read(&mut self, chain: &Chain, block: &Block, me: &[Address]) {
        let height = self.next_height;
        self.next_height += 1;
        let time_ms = block_time_ms(block);
        for tx in &block.txs {
            let txid = tx.id();
            self.index.add(height, time_ms, tx);
            let shape = shape_of(tx, me, |point| self.mine.remove(point));
            for (index, output) in tx.outputs.iter().enumerate() {
                if output.is_data() {
                    if self.notes.len() >= NOTES_SHOWN {
                        self.notes.pop_front();
                    }
                    self.notes.push_back(NoteRow { height, time_ms, text: text_of(&output.lock) });
                } else if let Some(own) = me.iter().find(|own| output.pays(&own[..])) {
                    self.mine.insert(OutPoint { txid, index: index as u32 }, (*own, output.value));
                }
            }
            let Some((kind, amount)) = kind_of(&shape, height) else {
                continue;
            };
            if kind == ActivityKind::Mined {
                self.blocks_mined += 1;
            }
            if self.items.len() >= KEPT {
                self.items.pop_front();
            }
            let ripe_at = (kind == ActivityKind::Mined).then(|| chain.config().rules_at(height).matures_at(height));
            self.items.push_back(Activity {
                kind,
                height: Some(height),
                amount,
                time_ms,
                id: txid,
                fee: shape.fee(),
                size: tx.encode().len() as u64,
                coins_in: shape.spent.len(),
                ripe_at,
            });
        }
    }

    /// The payments waiting in `pool`: the wallet's own as rows, and all of them for a list.
    /// A coin a waiting payment spends is the wallet's when the chain or an earlier waiting
    /// payment pays it to one of `me`.
    pub fn pool(&mut self, chain: &Chain, pool: &[Tx], me: &[Address], now_ms: u64) -> PoolSeen {
        let ids: Vec<Hash> = pool.iter().map(Tx::id).collect();
        self.first_seen.retain(|id, _| ids.contains(id));
        let waiting = chain.waiting(pool);
        let mut seen = PoolSeen::default();
        for (at, tx) in pool.iter().enumerate() {
            let since = *self.first_seen.entry(ids[at]).or_insert(now_ms);
            let (fee, size) = waiting.get(at).map_or((0, 0), |entry| (entry.fee, entry.size));
            let owner = |point: &OutPoint| {
                let output = match ids.iter().position(|id| *id == point.txid) {
                    Some(parent) => pool[parent].outputs.get(point.index as usize).cloned(),
                    None => chain.coin_at(point).map(|coin| coin.output.clone()),
                }?;
                me.iter().find(|own| output.pays(&own[..])).map(|own| (*own, output.value))
            };
            let shape = shape_of(tx, me, owner);
            let kind = kind_of(&shape, chain.height() + 1);
            seen.all.push(PendingRow { id: ids[at], fee, size, seen_ms: since, mine: kind.is_some() });
            if let Some((kind, amount)) = kind {
                let coins_in = shape.spent.len();
                let row = Activity { kind, height: None, amount, time_ms: since, id: ids[at], fee: Some(fee), size, coins_in, ripe_at: None };
                seen.mine.push(row);
            }
        }
        seen.mine.sort_by_key(|row| std::cmp::Reverse(row.time_ms));
        seen
    }

    /// `explored` with what the index knows: an address's payments, and the coins a payment
    /// spent that the chain no longer names.
    pub fn enrich(&self, mut explored: Explored) -> Explored {
        let fill = |tx: &mut super::explorer::TxRow| {
            for input in tx.ins.iter_mut().filter(|input| input.value.is_none()) {
                if let Some((address, value)) = self.index.coin(&input.previous) {
                    input.from = olc_wallet::format_address(&address);
                    input.value = Some(value);
                }
            }
            tx.settle_fee();
        };
        match &mut explored {
            Explored::Address { address, moves, indexed_from, .. } => {
                if let Some(found) = parse_address(address) {
                    *moves = self.index.of(&found);
                    *indexed_from = self.index.from();
                }
            }
            Explored::Tx { tx, .. } => fill(tx),
            Explored::Block(block) => block.txs.iter_mut().for_each(fill),
            _ => {}
        }
        explored
    }
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;
