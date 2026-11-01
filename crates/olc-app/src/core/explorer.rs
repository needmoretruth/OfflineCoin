//! Blocks, payments, addresses and notes, read from the node's own chain.
use olc_core::{Block, Chain, Hash};
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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TxRow {
    pub id: Hash,
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

pub fn text_of(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => bytes.iter().map(|byte| format!("{byte:02x}")).collect(),
    }
}

fn tx_row(tx: &olc_core::Tx) -> TxRow {
    let outputs = tx.outputs.iter().map(|output| {
        let data = output.is_data();
        let address = <[u8; 20]>::try_from(output.lock.as_slice()).ok();
        OutputRow {
            value: output.value,
            to: address.filter(|_| !data).map(|to| format_address(&to)).unwrap_or_default(),
            note: data.then(|| text_of(&output.lock)),
        }
    });
    TxRow {
        id: tx.id(),
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

fn parse_hash(text: &str) -> Option<Hash> {
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
