//! Blocks, payments, addresses and notes, read from the node's own chain.
use olc_core::{Block, Chain, Hash, ADDRESS_LANGUAGE};
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TxRow {
    pub id: Hash,
    /// 1 as the first Bitcoin wrote it, 2 for the form whose id leaves the signatures out.
    pub version: u32,
    pub is_coinbase: bool,
    pub inputs: usize,
    pub outputs: Vec<OutputRow>,
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
    },
}

/// Data as text when it reads as text, else as hex: a miner's counter of zero bytes is
/// valid text that shows nothing.
pub fn text_of(bytes: &[u8]) -> String {
    let readable = |text: &str| text.chars().all(|c| !c.is_control() || c == '\n' || c == '\t');
    match std::str::from_utf8(bytes) {
        Ok(text) if readable(text) => text.to_string(),
        _ => bytes.iter().map(|byte| format!("{byte:02x}")).collect(),
    }
}

fn tx_row(tx: &olc_core::Tx) -> TxRow {
    let outputs = tx.outputs.iter().map(|output| {
        let data = output.is_data();
        let address = <[u8; 20]>::try_from(output.lock.as_slice()).ok();
        let plain = address.filter(|_| output.language == ADDRESS_LANGUAGE);
        OutputRow {
            value: output.value,
            to: plain.map(|to| format_address(&to)).unwrap_or_default(),
            note: data.then(|| text_of(&output.lock)),
            lock: Dialect::of(output.language).map(|dialect| words_of_in(dialect, &output.lock)),
        }
    });
    TxRow {
        id: tx.id(),
        version: tx.version,
        is_coinbase: tx.is_coinbase(),
        inputs: tx.inputs.len(),
        outputs: outputs.collect(),
    }
}

pub fn block_row(height: u64, block: &Block) -> BlockRow {
    BlockRow {
        height,
        id: block.id(),
        time_ms: block.header.time_ms,
        payments: block.txs.len().saturating_sub(1),
        difficulty: block.header.difficulty,
    }
}

fn detail(height: u64, block: &Block) -> Explored {
    Explored::Block(Box::new(BlockDetail {
        row: block_row(height, block),
        previous: block.header.previous,
        txs: block.txs.iter().map(tx_row).collect(),
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
        Some(block) if height >= chain.oldest_held() => detail(height, block),
        _ => Explored::Pruned {
            height,
            oldest: chain.oldest_held(),
        },
    }
}

fn by_hash(chain: &Chain, id: &Hash) -> Explored {
    if let Some(block) = chain.block(id) {
        return detail(chain.height_of_block(id).unwrap_or(0), &block);
    }
    let Some(height) = chain.find_tx(id) else {
        return Explored::Nothing;
    };
    let blocks = chain.blocks_from(height, 1);
    let tx = blocks.first().and_then(|block| block.txs.iter().find(|tx| tx.id() == *id));
    tx.map_or(Explored::Nothing, |tx| Explored::Tx {
        height,
        tx: Box::new(tx_row(tx)),
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
    })
}

fn hex(hash: &Hash) -> String {
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A payment as lines: its id, where its coins came from, and each output.
fn tx_lines(tx: &TxRow, lines: &mut Vec<String>) {
    let source = if tx.is_coinbase { "mined".to_string() } else { format!("{} coins in", tx.inputs) };
    lines.push(format!("payment     {}  {source}, version {}", hex(&tx.id), tx.version));
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
                lines.push(format!("time_ms     {}", row.time_ms));
                lines.push(format!("difficulty  {}", row.difficulty));
                lines.push(format!("payments    {}", row.payments));
                block.txs.iter().for_each(|tx| tx_lines(tx, &mut lines));
            }
            Self::Tx { height, tx } => {
                tx_lines(tx, &mut lines);
                lines.push(format!("in block    {height}"));
            }
            Self::Address { address, balance, coins } => {
                lines.push(format!("address     {address}"));
                lines.push(format!("balance     {}", super::coins::format_coins(*balance)));
                lines.push(format!("coins       {coins}"));
            }
        }
        lines.join("\n")
    }
}
