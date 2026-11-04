//! The lines a board answers, spoken by a node process on this computer. An experiment written
//! for boards (the double spend in olc-lab) then runs on this computer's miners as it is, through
//! the same commands: `status`, `tx`, `pay`, `pool clear`, `mine`, `offline` and `limit`. Each
//! comes as `board <line>` and its answer is printed as a board prints it, beside the JSON lines.
use super::supervisor::ChainBlock;
use crate::core::{parse_coins, Host};
use olc_core::types::{Block, Tx};
use olc_core::coin::COIN;
use olc_core::Hash;
use olc_wallet::format_address;
use std::collections::{BTreeMap, HashMap, VecDeque};

/// Payments remembered after they leave the pool, so a dropped one can still say why.
const KEPT_TXS: usize = 512;
/// Heights watched for a branch change.
const WATCHED_HEIGHTS: u64 = 64;
/// Newest blocks in each status line, for the picture of the chain.
pub const SHOWN_BLOCKS: u64 = 14;

/// What a board keeps for its answers and a node process does not: payments it saw, and the
/// blocks it held a moment ago, so it can tell a branch change as a board does.
#[derive(Default)]
pub struct BoardSide {
    seen: HashMap<Hash, Tx>,
    order: VecDeque<Hash>,
    held: BTreeMap<u64, Hash>,
    /// The last branch change as (first replaced height, blocks replaced).
    reorg: (u64, u64),
    /// What it was last told. The node's own view lags a quarter second behind an order, and an
    /// experiment reads a setting back at once.
    mining: Option<bool>,
    cap: Option<u64>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhash(text: &str) -> Option<Hash> {
    let mut id: Hash = [0; 32];
    if text.len() != 64 {
        return None;
    }
    for (at, byte) in id.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(at * 2..at * 2 + 2)?, 16).ok()?;
    }
    Some(id)
}

/// Coins as a board prints them.
fn coins(units: u64) -> String {
    format!("{}.{:08}", units / COIN, units % COIN)
}

/// Who a block paid its reward to.
fn miner_of(block: &Block) -> String {
    let lock = block.txs.first().and_then(|coinbase| coinbase.outputs.first());
    lock.and_then(|output| <[u8; 20]>::try_from(output.lock.as_slice()).ok())
        .map(|address| format_address(&address))
        .unwrap_or_default()
}

impl BoardSide {
    fn keep(&mut self, tx: &Tx) {
        let id = tx.id();
        if self.seen.contains_key(&id) {
            return;
        }
        if self.order.len() == KEPT_TXS {
            if let Some(old) = self.order.pop_front() {
                self.seen.remove(&old);
            }
        }
        self.order.push_back(id);
        self.seen.insert(id, tx.clone());
    }

    /// Looks at the node once a turn: remembers the payments it holds and notices blocks that
    /// are no longer on its chain.
    pub fn watch(&mut self, host: &Host) {
        let Ok(runner) = host.runner.lock() else {
            return;
        };
        let pool: Vec<Tx> = runner.engine.pool().to_vec();
        let chain = runner.engine.chain();
        let height = chain.height();
        let replaced: Vec<u64> = self
            .held
            .iter()
            .filter(|(at, id)| chain.id_at(**at) != Some(**id))
            .map(|(at, _)| *at)
            .collect();
        if let Some(first) = replaced.first() {
            self.reorg = (*first, replaced.len() as u64);
        }
        let changed = self.held.get(&height) != chain.id_at(height).as_ref();
        let mut blocks = Vec::new();
        if changed {
            self.held.clear();
            for at in height.saturating_sub(WATCHED_HEIGHTS - 1)..=height {
                if let Some(id) = chain.id_at(at) {
                    self.held.insert(at, id);
                }
            }
            let newest = height.saturating_sub(SHOWN_BLOCKS - 1).max(1)..=height;
            blocks = newest.filter_map(|at| chain.block(&chain.id_at(at)?)).collect();
        }
        drop(runner);
        pool.iter().for_each(|tx| self.keep(tx));
        for block in &blocks {
            block.txs.iter().skip(1).for_each(|tx| self.keep(tx));
        }
    }

    /// Answers one board line, or says nothing for a line a board would not answer.
    pub fn answer(&mut self, host: &Host, line: &str) -> Option<String> {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            ["status"] => return Some(self.status(host)),
            ["tx", id] => return Some(self.place(host, id)),
            ["pay", to, amount] => {
                let paid = parse_coins(amount).map(|units| host.pay(to, units));
                return Some(match paid {
                    Some(Ok(id)) => format!("paid {}", hex(&id)),
                    Some(Err(error)) => format!("error {}", error.notice().0),
                    None => "error amount".to_string(),
                });
            }
            ["pool", "clear"] => {
                if let Ok(mut runner) = host.runner.lock() {
                    runner.engine.clear_pool();
                }
            }
            ["mine", on] => {
                self.mining = Some(*on == "on");
                host.set_mining(*on == "on");
            }
            ["offline", on] => {
                if let Ok(mut runner) = host.runner.lock() {
                    runner.offline = *on == "on";
                }
            }
            // Mining rests at this height, 0 for never: the Lab makes the blocks asked and no more.
            ["mine-to", height] => host.mine_to(height.parse().ok().filter(|height| *height > 0)),
            ["limit", cap] => {
                if let Ok(cap) = cap.parse() {
                    self.cap = Some(cap);
                    host.set_cap(cap);
                }
            }
            _ => {}
        }
        None
    }

    fn status(&mut self, host: &Host) -> String {
        self.watch(host);
        let view = host.view();
        let (offline, height, tip, work, pool) = match host.runner.lock() {
            Ok(runner) => {
                let chain = runner.engine.chain();
                let tip = hex(&chain.tip());
                (runner.offline, chain.height(), tip, chain.total_work(), runner.engine.pool().len())
            }
            Err(_) => return "error stopped".into(),
        };
        format!(
            "status height={} tip={} work={} balance={} pool={} hashrate={} limit={} chosen=true \
             mining={} offline={} address={} reorg={}/{}",
            height,
            tip,
            work,
            coins(view.spendable),
            pool,
            view.hashrate,
            self.cap.unwrap_or(view.cap),
            self.mining.unwrap_or(view.mining),
            offline,
            view.address,
            self.reorg.0,
            self.reorg.1,
        )
    }

    /// Where this node sees a payment, in the words a board uses. A payment this node held that
    /// is now neither in a block nor waiting was dropped, and the chain says why.
    fn place(&mut self, host: &Host, text: &str) -> String {
        self.watch(host);
        let Some(id) = unhash(text) else {
            return "error tx ID".into();
        };
        let Ok(runner) = host.runner.lock() else {
            return "error tx ID".into();
        };
        let chain = runner.engine.chain();
        let place = match chain.find_tx(&id) {
            Some(height) => format!("height={height}"),
            None if runner.engine.in_pool(&id) => "pool".into(),
            None => match self.seen.get(&id).map(|tx| chain.check_tx(tx, chain.height() + 1)) {
                Some(Err(reason)) => format!("dropped=Some({reason:?})"),
                _ => "none".into(),
            },
        };
        format!("tx {text} {place}")
    }

    /// The newest blocks on this node's chain, for the picture of the race.
    pub fn newest_blocks(host: &Host) -> Vec<ChainBlock> {
        let Ok(runner) = host.runner.lock() else {
            return Vec::new();
        };
        let chain = runner.engine.chain();
        let height = chain.height();
        let first = height.saturating_sub(SHOWN_BLOCKS - 1).max(1);
        (first..=height)
            .filter_map(|at| {
                let block = chain.block(&chain.id_at(at)?)?;
                Some(ChainBlock {
                    height: at,
                    id: hex(&block.id()),
                    miner: miner_of(&block),
                    txs: block.txs.iter().skip(1).map(|tx| hex(&tx.id())).collect(),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coins_read_as_a_board_prints_them() {
        assert_eq!(coins(50 * COIN + 5), "50.00000005");
        assert_eq!(coins(0), "0.00000000");
    }

    #[test]
    fn an_id_reads_back() {
        let id: Hash = [0xab; 32];
        assert_eq!(unhash(&hex(&id)), Some(id));
        assert_eq!(unhash("ab"), None);
    }
}
