//! What a Bitcoin Core user asks with getrawmempool, listunspent and generatetoaddress, as this
//! node answers it: the payments waiting for a block, this wallet's coins, and blocks made now by
//! real mining at full speed on the coin in use, counted until this node made as many as asked.
use super::coins::{format_coins, group_thousands};
use super::host::Host;
use crate::settings::standard_threads;
use olc_core::{Address, Chain, Hash, Tx};
use olc_node::Runner;
use olc_wallet::format_address;
use std::sync::MutexGuard;

/// The most blocks one `generate` makes, so a typo of a few zeros does not mine for a day.
pub const MOST_TO_MAKE: u32 = 1_000;

/// What a person asks about the chain from a console.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChainOrder {
    /// The payments waiting for a block, as getrawmempool.
    Waiting,
    /// This wallet's coins, as listunspent.
    Coins,
    /// Mines at full speed until this node made this many more blocks, as generatetoaddress.
    Make(u32),
    /// How far the making is.
    MakeShow,
    MakeStop,
}

fn hex(hash: &Hash) -> String {
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn count(value: u64) -> String {
    group_thousands(u128::from(value))
}

/// Each waiting payment with its size, fee and the waiting payments it needs first.
pub fn waiting_listing(chain: &Chain, pool: &[Tx]) -> String {
    if pool.is_empty() {
        return "No payments are waiting for a block.".into();
    }
    let waiting = chain.waiting(pool);
    let bytes: u64 = waiting.iter().map(|one| one.size).sum();
    let fees: u64 = waiting.iter().map(|one| one.fee).sum();
    let mut lines = vec![format!("{} waiting for a block, {} bytes, {} in fees.", payments(pool.len()), count(bytes), format_coins(fees))];
    for (tx, one) in pool.iter().zip(&waiting) {
        let after = match one.parents.len() {
            0 => String::new(),
            1 => ", after 1 waiting payment".into(),
            parents => format!(", after {parents} waiting payments"),
        };
        lines.push(format!("  {}  {} bytes  fee {}{after}", hex(&tx.id()), count(one.size), format_coins(one.fee)));
    }
    lines.join("\n")
}

fn payments(count: usize) -> String {
    match count {
        1 => "1 payment".into(),
        count => format!("{count} payments"),
    }
}

/// Every coin the wallet's addresses hold, newest first: where it is, how much, how many blocks
/// stand on it and the address it pays. A mined coin that cannot move yet says when it can.
pub fn coins_listing(chain: &Chain, addresses: &[Address]) -> String {
    let tip = chain.height();
    let mut rows: Vec<_> = addresses.iter().flat_map(|address| chain.unspent_of(address).into_iter().map(move |(point, entry)| (point, entry, *address))).collect();
    if rows.is_empty() {
        return "This wallet has no coins yet.".into();
    }
    rows.sort_by(|one, other| other.1.height.cmp(&one.1.height).then(one.0.cmp(&other.0)));
    let total: u64 = rows.iter().map(|(_, entry, _)| entry.output.value).sum();
    let noun = if rows.len() == 1 { "coin" } else { "coins" };
    let mut lines = vec![format!("{} {noun}, {} in all.", rows.len(), format_coins(total))];
    for (point, entry, address) in &rows {
        let confirmations = tip.saturating_sub(entry.height) + 1;
        let blocks = if confirmations == 1 { "confirmation" } else { "confirmations" };
        let ripe = match chain.is_mature(entry, tip + 1) {
            true => String::new(),
            false => {
                let at = chain.config().rules_at(entry.height).matures_at(entry.height);
                format!("  mined, moves in {} blocks", at.saturating_sub(tip + 1).max(1))
            }
        };
        // Two lines a coin, so each fits a window's width: what it is worth, then where it is.
        let worth = format_coins(entry.output.value);
        lines.push(format!("  {worth}  {} {blocks}  {}{ripe}", count(confirmations), format_address(address)));
        lines.push(format!("    {}:{}", hex(&point.txid), point.index));
    }
    lines.join("\n")
}

/// Blocks being made now: how many were asked for and those this node made so far.
#[derive(Clone, Debug)]
pub struct Making {
    pub asked: u32,
    pub made: Vec<(u64, Hash)>,
    /// The address mined blocks pay, so blocks from others on the same chain do not count.
    pub miner: Address,
    pub genesis: Hash,
    /// The tip's height at the last look.
    looked: u64,
    /// How this computer mined before, to go back to.
    pub was: Was,
}

/// How a node mined before making blocks: on or off, its cap, its threads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Was {
    pub mining: bool,
    pub cap: u64,
    pub threads: usize,
}

/// A notice: its key, its detail, and whether it went well.
pub type Said = (&'static str, String, bool);

/// The lists the reading words give, from the node as it stands.
pub fn read(host: &Host, order: &ChainOrder) -> Option<Said> {
    let listed = |runner: MutexGuard<Runner>| match order {
        ChainOrder::Waiting => Some(("notice.waiting_list", waiting_listing(runner.engine.chain(), runner.engine.pool()), true)),
        ChainOrder::Coins => {
            let addresses: Vec<Address> = runner.engine.wallet().keys().map(|(address, _)| address).collect();
            Some(("notice.coins_list", coins_listing(runner.engine.chain(), &addresses), true))
        }
        _ => None,
    };
    match host.ready() {
        Ok(runner) => listed(runner),
        Err(detail) => matches!(order, ChainOrder::Waiting | ChainOrder::Coins).then_some(("notice.chain_closed", detail, false)),
    }
}

/// Blocks made on a node: started, followed each turn, stopped. OLC Core and `olc node` share it.
#[derive(Default)]
pub struct Maker {
    making: Option<Making>,
    /// How the last making ended, for `generate` once it is over.
    last: Option<String>,
}

impl Maker {
    pub fn busy(&self) -> bool {
        self.making.is_some()
    }

    /// Carries out a making order and says what came of it.
    pub fn order(&mut self, host: &Host, order: ChainOrder) -> Said {
        match order {
            ChainOrder::Make(asked) => self.start(host, asked),
            ChainOrder::MakeShow => {
                let text = self.making.as_ref().map(Making::listing).or_else(|| self.last.clone());
                ("notice.making_status", text.unwrap_or_else(|| "No blocks are being made.".into()), true)
            }
            ChainOrder::MakeStop => match self.making.take() {
                Some(making) => {
                    mine_as_before(host, making.was);
                    ("notice.making_stopped", making.listing(), true)
                }
                None => ("notice.not_making", String::new(), false),
            },
            other => read(host, &other).unwrap_or(("notice.not_changed", String::new(), false)),
        }
    }

    fn start(&mut self, host: &Host, asked: u32) -> Said {
        if !(1..=MOST_TO_MAKE).contains(&asked) {
            return ("notice.making_count", String::new(), false);
        }
        if let Some(making) = &self.making {
            return ("notice.making_already", making.listing(), false);
        }
        let view = host.view();
        let was = Was { mining: view.mining, cap: view.cap, threads: host.threads() };
        let making = match host.ready() {
            Ok(runner) if runner.engine.light().is_some() => return ("notice.making_light", String::new(), false),
            Ok(runner) => Making::new(asked, runner.engine.chain(), runner.engine.miner(), was),
            Err(detail) => return ("notice.chain_closed", detail, false),
        };
        // Rests at the asked height, so a coin that finds many blocks a second makes no more.
        host.mine_to(Some(making.looked + u64::from(asked)));
        host.set_cap(0);
        host.set_threads(standard_threads());
        host.set_mining(true);
        self.making = Some(making);
        ("notice.making", asked.to_string(), true)
    }

    /// One turn: the newest block this node made since the last, and the end once all are made.
    pub fn step(&mut self, host: &Host) -> Vec<Said> {
        let Some(mut making) = self.making.take() else {
            return Vec::new();
        };
        let looked = host.ready().ok().map(|runner| {
            let chain = runner.engine.chain();
            (chain.genesis_id() == making.genesis).then(|| making.look(chain))
        });
        let mut said = Vec::new();
        match looked {
            // Still opening: looked at next turn.
            None => self.making = Some(making),
            // The coin changed under it: what was made stays on the other coin.
            Some(None) => {
                mine_as_before(host, making.was);
                said.push(("notice.making_stopped", making.listing(), true));
            }
            Some(Some(new)) => {
                for (at, (height, _)) in new.iter().enumerate() {
                    let counted = making.made.len() - new.len() + at + 1;
                    said.push(("notice.made_block", format!("{} ({counted}/{})", count(*height), making.asked), true));
                }
                // Blocks another node found meanwhile do not count: mining goes on past them.
                let left = u64::from(making.asked).saturating_sub(making.made.len() as u64);
                host.mine_to(Some(making.looked + left));
                if making.done() {
                    mine_as_before(host, making.was);
                    let text = making.listing();
                    self.last = Some(text.clone());
                    said.push(("notice.made", text, true));
                } else {
                    self.making = Some(making);
                }
            }
        }
        said
    }
}

fn mine_as_before(host: &Host, was: Was) {
    host.mine_to(None);
    host.set_cap(was.cap);
    host.set_threads(was.threads);
    host.set_mining(was.mining);
}

impl Making {
    pub fn new(asked: u32, chain: &Chain, miner: Address, was: Was) -> Self {
        Self { asked, made: Vec::new(), miner, genesis: chain.genesis_id(), looked: chain.height(), was }
    }

    /// Takes in the blocks added since the last look and gives those this node made. A block of
    /// this node's that another replaced no longer counts, and its height is looked at again.
    pub fn look(&mut self, chain: &Chain) -> Vec<(u64, Hash)> {
        let tip = chain.height();
        let lost = self.made.iter().filter(|(height, id)| chain.id_at(*height) != Some(*id)).map(|(height, _)| *height).min();
        self.made.retain(|(height, id)| chain.id_at(*height) == Some(*id));
        let from = lost.map_or(self.looked, |height| height.saturating_sub(1)).min(self.looked).min(tip);
        let mut new = Vec::new();
        for height in from + 1..=tip {
            if self.done() {
                break;
            }
            let Some(id) = chain.id_at(height) else { continue };
            if self.made.iter().any(|(_, known)| *known == id) {
                continue;
            }
            let block = chain.block(&id);
            let coinbase = block.as_ref().and_then(|block| block.txs.first());
            if coinbase.is_some_and(|tx| tx.outputs.iter().any(|output| output.pays(&self.miner[..]))) {
                self.made.push((height, id));
                new.push((height, id));
            }
        }
        self.looked = tip;
        new
    }

    pub fn done(&self) -> bool {
        self.made.len() >= self.asked as usize
    }

    /// How far it is, in lines: one for each block made.
    pub fn listing(&self) -> String {
        let mut lines = vec![format!("{} of {} blocks made.", self.made.len(), self.asked)];
        lines.extend(self.made.iter().map(|(height, id)| format!("  block {}  {}", count(*height), hex(id))));
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use olc_core::{Block, CoinConfig, Consensus, Plugins};
    use olc_pow::ProofOfWork;
    use olc_wallet::Wallet;

    fn address(secret: u8) -> Address {
        Wallet::from_secret([secret; 32]).unwrap().address()
    }

    /// A chain on a coin mined in a moment, with blocks paying `miners` in turn.
    fn chain_of(miners: &[Address]) -> Chain {
        let mut coin = CoinConfig::olc();
        coin.initial_difficulty = 8;
        coin.rules.retarget_interval = 0;
        coin.rules.coinbase_maturity = 3;
        let mut chain = Chain::new(coin, Plugins { consensus: Box::new(ProofOfWork), languages: olc_lang::all() });
        for miner in miners {
            let block = mined(&chain, miner);
            chain.add_block(block).unwrap();
        }
        chain
    }

    fn mined(chain: &Chain, miner: &Address) -> Block {
        let mut block = chain.block_template(miner, &[], chain.tip_header().time_ms + 1_000);
        assert!(ProofOfWork.try_seal(&mut block.header, 1_000_000));
        block
    }

    #[test]
    fn only_blocks_this_node_made_count_and_it_stops_at_the_number_asked() {
        let (mine, other) = (address(1), address(2));
        let mut chain = chain_of(&[mine]);
        let mut making = Making::new(2, &chain, mine, Was::default());
        for miner in [other, mine, other, mine, mine] {
            let block = mined(&chain, &miner);
            chain.add_block(block).unwrap();
        }
        let made = making.look(&chain);
        assert_eq!(made.iter().map(|(height, _)| *height).collect::<Vec<_>>(), [3, 5], "blocks 2 and 4 are another's, 6 is past two");
        assert!(making.done());
        assert!(making.listing().starts_with("2 of 2 blocks made."), "{}", making.listing());
    }

    #[test]
    fn coins_say_amount_confirmations_address_and_when_mined_ones_move() {
        let mine = address(1);
        let chain = chain_of(&[mine, mine]);
        let listed = coins_listing(&chain, &[mine]);
        let lines: Vec<&str> = listed.lines().collect();
        assert!(lines[0].starts_with("2 coins, "), "{listed}");
        assert!(lines[1].contains("1 confirmation ") && lines[1].contains("mined, moves in"), "{listed}");
        assert!(lines[2].trim_start().len() > 64 && lines[2].contains(':'), "{listed}");
        assert!(lines[3].contains("2 confirmations") && lines[3].contains(&format_address(&mine)), "{listed}");
        assert_eq!(coins_listing(&chain, &[address(3)]), "This wallet has no coins yet.");
        assert_eq!(waiting_listing(&chain, &[]), "No payments are waiting for a block.");
    }
}
