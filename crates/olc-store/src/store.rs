//! Opening a node's chain on a computer. Blocks live in segment files behind the same log a board
//! uses. Blocks an older version left in `blocks.log` join the new store once.
use crate::files::keep_in;
use olc_core::{Block, Chain, CoinConfig, Plugins};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

const OLD_BLOCKS: &str = "blocks.log";
const OLD_BLOCKS_DONE: &str = "blocks.log.imported";
const SETTINGS: &str = "node.json";

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
