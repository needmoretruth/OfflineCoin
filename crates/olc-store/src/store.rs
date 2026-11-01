//! Opening a node's chain on a computer. Blocks live in segment files behind the same log a board
//! uses. Blocks an older version left in `blocks.log` join the new store once.
use crate::files::keep_in;
use olc_core::{Block, BlockHeader, Chain, CoinConfig, Plugins};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

const OLD_BLOCKS: &str = "blocks.log";
const OLD_BLOCKS_DONE: &str = "blocks.log.imported";
const SETTINGS: &str = "node.json";
const HEADERS: &str = "headers.bin";
const HEADERS_NEW: &str = "headers.new";

/// What a person chose for this node.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeSettings {
    /// Recent blocks to keep. `None` keeps everything, because a computer has the space.
    #[serde(default)]
    pub recent: Option<u64>,
    /// Address mined coins go to. `None` is the key that receives.
    #[serde(default)]
    pub miner: Option<String>,
    /// Text every mined block carries.
    #[serde(default)]
    pub note: String,
    /// Leaves small coins as they are when set.
    #[serde(default)]
    pub no_merge: bool,
    /// The order links are preferred in, such as `wired,espnow,bluetooth,wifi`. Empty means the
    /// usual order.
    #[serde(default)]
    pub links: String,
    /// Keeps headers only, and asks full nodes for its coins.
    #[serde(default)]
    pub light: bool,
    /// Mines from the start. Off unless the owner turns it on, as in Bitcoin Core.
    #[serde(default)]
    pub mining: bool,
    /// Addresses followed without a key.
    #[serde(default)]
    pub watched: Vec<String>,
}

impl NodeSettings {
    pub fn load(directory: &Path) -> Self {
        let text = fs::read_to_string(directory.join(SETTINGS)).unwrap_or_default();
        serde_json::from_str(&text).unwrap_or_default()
    }

    pub fn save(&self, directory: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|error| error.to_string())?;
        fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        fs::write(directory.join(SETTINGS), text).map_err(|error| error.to_string())
    }
}

/// The headers a light node saved. Each one is checked again as it is taken back.
pub fn load_headers(directory: &Path) -> Vec<BlockHeader> {
    let bytes = fs::read(directory.join(HEADERS)).unwrap_or_default();
    let (headers, _) = bytes.as_chunks::<{ BlockHeader::ENCODED_LEN }>();
    headers.iter().map_while(|header| BlockHeader::decode(header)).collect()
}

/// Replaces the saved headers whole, so a cut while saving leaves the last ones.
pub fn save_headers(directory: &Path, headers: &[BlockHeader]) -> Result<(), String> {
    let bytes: Vec<u8> = headers.iter().flat_map(BlockHeader::encode).collect();
    let fresh = directory.join(HEADERS_NEW);
    fs::write(&fresh, bytes).map_err(|error| error.to_string())?;
    fs::rename(&fresh, directory.join(HEADERS)).map_err(|error| error.to_string())
}

/// Opens the chain in `directory`, with `limit` bytes of disk if it has a limit.
pub fn open_chain(
    directory: &Path,
    config: CoinConfig,
    plugins: Plugins,
    limit: Option<u64>,
) -> Result<Chain, String> {
    let recent = NodeSettings::load(directory).recent;
    let keep = keep_in(directory, recent, limit)?;
    let mut chain = Chain::open(config, plugins, keep);
    let old = directory.join(OLD_BLOCKS);
    if old.exists() {
        for block in read_blocks(&old) {
            let _ = chain.add_block(block);
        }
        chain.save_now();
        fs::rename(&old, directory.join(OLD_BLOCKS_DONE)).map_err(|error| error.to_string())?;
    }
    Ok(chain)
}

fn read_blocks(path: &Path) -> Vec<Block> {
    let Ok(data) = fs::read(path) else {
        return Vec::new();
    };
    let mut blocks = Vec::new();
    let mut rest = data.as_slice();
    while rest.len() >= 4 {
        let size = u32::from_le_bytes([rest[0], rest[1], rest[2], rest[3]]) as usize;
        let Some(bytes) = rest.get(4..4 + size) else {
            break;
        };
        match postcard::from_bytes(bytes) {
            Ok(block) => blocks.push(block),
            Err(_) => break,
        }
        rest = &rest[4 + size..];
    }
    blocks
}
