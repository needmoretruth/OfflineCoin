//! Blocks, payments, addresses and notes, read from the node's own chain.
use super::address_index::AddressMove;
use olc_core::{Block, Chain, Hash, OutPoint, Tx, TxOut, ADDRESS_LANGUAGE};
use olc_lang::words::{words_of_in, Dialect};
use olc_wallet::{format_address, parse_address};

/// Enough to fill a screen, and little enough to read at every new tip.
pub const RECENT_BLOCKS: u64 = 30;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BlockRow {
    pub height: u64,
    pub id: Hash,
    pub time_ms: u64,
    pub payments: usize,
    pub difficulty: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputRow {
    pub value: u64,
    pub to: String,
    /// The words of a data output.
    pub note: Option<String>,
    /// The words of a beta or gamma lock.
    pub lock: Option<String>,
}

/// A coin a payment spends: what it was, once this node can tell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputRow {
    pub previous: OutPoint,
    /// Its address, empty under a lock or when unknown here.
    pub from: String,
    pub value: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TxRow {
    pub id: Hash,
    /// 1 as the first Bitcoin wrote it, 2 for the form whose id leaves the signatures out.
    pub version: u32,
    pub is_coinbase: bool,
    pub inputs: usize,
    pub ins: Vec<InputRow>,
    pub outputs: Vec<OutputRow>,
    /// Known when every coin it spends is.
    pub fee: Option<u64>,
    pub size: u64,
}

impl TxRow {
    /// Works the fee out again, once every input's value is known.
    pub fn settle_fee(&mut self) {
        let spent: Option<u64> = self.ins.iter().map(|input| input.value).sum();
        let made: u64 = self.outputs.iter().map(|output| output.value).sum();
        self.fee = if self.is_coinbase { None } else { spent.map(|spent| spent.saturating_sub(made)) };
    }
}

/// A payment waiting for a block, as the pool list shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingRow {
    pub id: Hash,
    pub fee: u64,
    pub size: u64,
    /// When this node first saw it.
    pub seen_ms: u64,
    /// Whether it touches this wallet.
    pub mine: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoteRow {
    pub height: u64,
    pub time_ms: u64,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockDetail {
    pub row: BlockRow,
    pub previous: Hash,
    /// The block after it on the best chain, once there is one.
    pub next: Option<Hash>,
    /// Where its reward went: the address that mined it.
    pub miner: String,
    pub size: u64,
    /// What its payments paid to be in it.
    pub fees: u64,
    pub txs: Vec<TxRow>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Explored {
    #[default]
    Nothing,
    /// The body of this block is gone from this node. `oldest` is the first it holds.
    Pruned {
        height: u64,
        oldest: u64,
    },
    Block(Box<BlockDetail>),
    Tx {
        height: u64,
        tx: Box<TxRow>,
    },
    Address {
        address: String,
        balance: u64,
        coins: usize,
        /// Its payments over the blocks read here, newest first. See [`super::address_index`].
        moves: Vec<AddressMove>,
        /// The first block whose payments `moves` can show.
        indexed_from: u64,
    },
}

/// When a block was made, in milliseconds. The one place that reads the header's time.
pub fn block_time_ms(block: &Block) -> u64 {
    block.header.time_ms
}

fn plain_address(output: &TxOut) -> String {
    let address = <[u8; 20]>::try_from(output.lock.as_slice()).ok();
    address.filter(|_| output.language == ADDRESS_LANGUAGE).map(|to| format_address(&to)).unwrap_or_default()
}

/// The output a coin was, while the chain still knows it: unspent, or in a recent payment.
fn spent_output(chain: &Chain, point: &OutPoint) -> Option<TxOut> {
    if let Some(coin) = chain.coin_at(point) {
        return Some(coin.output.clone());
    }
    let height = chain.find_tx(&point.txid)?;
    let blocks = chain.blocks_from(height, 1);
    let tx = blocks.first()?.txs.iter().find(|tx| tx.id() == point.txid)?;
    tx.outputs.get(point.index as usize).cloned()
}

pub fn text_of(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => bytes.iter().map(|byte| format!("{byte:02x}")).collect(),
    }
}

fn tx_row(chain: &Chain, tx: &Tx) -> TxRow {
    let outputs = tx.outputs.iter().map(|output| OutputRow {
        value: output.value,
        to: plain_address(output),
        note: output.is_data().then(|| text_of(&output.lock)),
        lock: Dialect::of(output.language).map(|dialect| words_of_in(dialect, &output.lock)),
    });
    let ins = tx.inputs.iter().map(|input| {
        let spent = spent_output(chain, &input.previous);
        InputRow {
            previous: input.previous,
            from: spent.as_ref().map(plain_address).unwrap_or_default(),
            value: spent.map(|output| output.value),
        }
    });
    let mut row = TxRow {
        id: tx.id(),
        version: tx.version,
        is_coinbase: tx.is_coinbase(),
        inputs: tx.inputs.len(),
        ins: ins.collect(),
        outputs: outputs.collect(),
        fee: None,
        size: tx.encode().len() as u64,
    };
    row.settle_fee();
    row
}

pub fn block_row(height: u64, block: &Block) -> BlockRow {
    BlockRow {
        height,
        id: block.id(),
        time_ms: block_time_ms(block),
        payments: block.txs.len().saturating_sub(1),
        difficulty: block.header.difficulty,
    }
}

fn detail(chain: &Chain, height: u64, block: &Block) -> Explored {
    let coinbase = block.txs.first();
    let paid = coinbase.map_or(0, |tx| tx.outputs.iter().map(|output| output.value).sum::<u64>());
    let reward = chain.config().rules_at(height).reward_at(height);
    let miner = coinbase.and_then(|tx| tx.outputs.iter().map(plain_address).find(|to| !to.is_empty()));
    Explored::Block(Box::new(BlockDetail {
        row: block_row(height, block),
        previous: block.header.previous,
        next: chain.id_at(height + 1),
        miner: miner.unwrap_or_default(),
        size: block.size() as u64,
        fees: paid.saturating_sub(reward),
        txs: block.txs.iter().map(|tx| tx_row(chain, tx)).collect(),
    }))
}

/// The newest blocks of the best chain, newest first.
pub fn recent_blocks(chain: &Chain) -> Vec<BlockRow> {
    let first = (chain.height() + 1).saturating_sub(RECENT_BLOCKS).max(chain.oldest_held());
    let blocks = chain.blocks_from(first, RECENT_BLOCKS as usize);
    let rows = blocks.iter().enumerate();
    let mut rows: Vec<BlockRow> = rows.map(|(at, b)| block_row(first + at as u64, b)).collect();
    rows.reverse();
    rows
}

fn at_height(chain: &Chain, height: u64) -> Explored {
    if height > chain.height() {
        return Explored::Nothing;
    }
    match chain.blocks_from(height, 1).first() {
        Some(block) if height >= chain.oldest_held() => detail(chain, height, block),
        _ => Explored::Pruned {
            height,
            oldest: chain.oldest_held(),
        },
    }
}

fn by_hash(chain: &Chain, id: &Hash) -> Explored {
    if let Some(block) = chain.block(id) {
        return detail(chain, chain.height_of_block(id).unwrap_or(0), &block);
    }
    let Some(height) = chain.find_tx(id) else {
        return Explored::Nothing;
    };
    let blocks = chain.blocks_from(height, 1);
    let tx = blocks.first().and_then(|block| block.txs.iter().find(|tx| tx.id() == *id));
    tx.map_or(Explored::Nothing, |tx| Explored::Tx {
        height,
        tx: Box::new(tx_row(chain, tx)),
    })
}

pub(crate) fn parse_hash(text: &str) -> Option<Hash> {
    let mut hash = [0; 32];
    if text.len() != 64 || !text.is_ascii() {
        return None;
    }
    for (at, byte) in hash.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[at * 2..at * 2 + 2], 16).ok()?;
    }
    Some(hash)
}

/// Finds a height, a block or payment id, or an address.
pub fn explore(chain: &Chain, text: &str) -> Explored {
    let text = text.trim();
    if let Ok(height) = text.parse() {
        return at_height(chain, height);
    }
    if let Some(id) = parse_hash(text) {
        return by_hash(chain, &id);
    }
    parse_address(text).map_or(Explored::Nothing, |address| Explored::Address {
        address: format_address(&address),
        balance: chain.balance(&address),
        coins: chain.unspent_of(&address).len(),
        moves: Vec::new(),
        indexed_from: chain.oldest_held(),
    })
}

fn hex(hash: &Hash) -> String {
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A payment as lines: its id, where its coins came from, and each output.
fn tx_lines(tx: &TxRow, lines: &mut Vec<String>) {
    let source = if tx.is_coinbase { "mined".to_string() } else { format!("{} coins in", tx.inputs) };
    lines.push(format!("payment     {}  {source}, version {}", hex(&tx.id), tx.version));
    let fee = tx.fee.map_or_else(|| "unknown".to_string(), super::coins::format_coins);
    lines.push(format!("  size {} bytes, fee {fee}", tx.size));
    for input in &tx.ins {
        let value = input.value.map_or_else(|| "?".to_string(), super::coins::format_coins);
        let from = if input.from.is_empty() { "a lock or an older coin" } else { input.from.as_str() };
        lines.push(format!("  in {value} from {from}"));
    }
    for output in &tx.outputs {
        let amount = super::coins::format_coins(output.value);
        let line = match (&output.note, &output.lock) {
            (Some(note), _) => format!("  note {note}"),
            (None, Some(lock)) => format!("  {amount} under the lock {lock}"),
            (None, None) if output.to.is_empty() => format!("  {amount} under a lock"),
            (None, None) => format!("  {amount} to {}", output.to),
        };
        lines.push(line);
    }
}

impl Explored {
    /// What was found, as lines a person reads. `asked` is what they typed.
    pub fn text(&self, asked: &str) -> String {
        let mut lines = Vec::new();
        match self {
            Self::Nothing => lines.push(format!("Nothing here is called {}.", asked.trim())),
            Self::Pruned { height, oldest } => lines.push(format!(
                "Block {height} is no longer kept here. The oldest kept is {oldest}."
            )),
            Self::Block(block) => {
                let row = &block.row;
                lines.push(format!("block       {}", row.height));
                lines.push(format!("id          {}", hex(&row.id)));
                lines.push(format!("previous    {}", hex(&block.previous)));
                if let Some(next) = &block.next {
                    lines.push(format!("next        {}", hex(next)));
                }
                lines.push(format!("time_ms     {}", row.time_ms));
                lines.push(format!("miner       {}", block.miner));
                lines.push(format!("size        {} bytes", block.size));
                lines.push(format!("fees        {}", super::coins::format_coins(block.fees)));
                lines.push(format!("difficulty  {}", row.difficulty));
                lines.push(format!("payments    {}", row.payments));
                block.txs.iter().for_each(|tx| tx_lines(tx, &mut lines));
            }
            Self::Tx { height, tx } => {
                tx_lines(tx, &mut lines);
                lines.push(format!("in block    {height}"));
            }
            Self::Address { address, balance, coins, moves, indexed_from } => {
                lines.push(format!("address     {address}"));
                lines.push(format!("balance     {}", super::coins::format_coins(*balance)));
                lines.push(format!("coins       {coins}"));
                lines.push(format!("payments    {} since block {indexed_from}", moves.len()));
                for row in moves {
                    let (came, left) = (super::coins::format_coins(row.received), super::coins::format_coins(row.spent));
                    lines.push(format!("  block {}  {}  in {came}  out {left}", row.height, hex(&row.id)));
                }
            }
        }
        lines.join("\n")
    }
}
