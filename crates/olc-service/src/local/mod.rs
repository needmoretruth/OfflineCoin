//! Several real nodes on one computer. Each is its own operating-system process running the same
//! node code: real hashing, real blocks and signatures, real validation, its own folder, and TCP
//! between them. What it is given is enforced by the operating system, and what it uses is
//! measured from the operating system, so the two can stand side by side.
mod board_lines;
mod child;
pub mod lab;
pub mod lab_net;
mod lab_picture;
mod lab_room;
pub mod lab_side;
mod measure;
mod preset;
mod spawn;
mod supervisor;
pub mod trouble;

pub use child::run_node;
pub use measure::{is_running, Measured};
pub use preset::{Preset, PROCESS_OVERHEAD_MIB};
pub use spawn::Enforcement;
pub use supervisor::{address_of, ChainBlock, ChildStatus, LocalLink, LocalView, Supervisor};

use olc_store::NodeSettings;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Whose wallet a node mines to and pays from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WalletChoice {
    /// A key in the node's own folder.
    #[default]
    Own,
    /// The app's wallet, so mining adds up in one place.
    Shared,
}

/// What a node keeps. One choice, as in Bitcoin Core: the person decides, no device decides.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Holding {
    /// Every block.
    #[default]
    Everything,
    /// The newest blocks, and the unspent set.
    Recent,
    /// Headers only.
    Headers,
}

impl Holding {
    pub fn key(self) -> &'static str {
        match self {
            Self::Everything => "local.keep_all",
            Self::Recent => "local.keep_recent",
            Self::Headers => "local.keep_headers",
        }
    }
}

/// One node process as a person set it up.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NodeSpec {
    pub id: u32,
    pub name: String,
    pub preset: Preset,
    /// Hashes per second. 0 is no cap, which leaves the CPU share as the limit.
    pub hash_cap: u64,
    /// Memory for chain data. The process itself takes `PROCESS_OVERHEAD_MIB` on top.
    pub memory_mib: u64,
    /// Percent of one core. 0 is no limit.
    pub cpu_percent: u32,
    pub disk_mib: u64,
    pub wallet: WalletChoice,
    /// Connected to the app's node and the other joined nodes. Off, it keeps a chain to itself.
    pub joined: bool,
    pub listen_lan: bool,
    pub mining: bool,
    pub holding: Holding,
    /// How many blocks [`Holding::Recent`] keeps.
    pub recent: u64,
}

impl NodeSpec {
    /// Writes what this node keeps where its process reads it.
    pub fn apply_holding(&self, folder: &Path) -> Result<(), String> {
        let mut settings = NodeSettings::load(folder);
        settings.light = self.holding == Holding::Headers;
        settings.recent = (self.holding == Holding::Recent).then_some(self.recent);
        settings.save(folder)
    }
}

impl Default for NodeSpec {
    fn default() -> Self {
        Preset::Esp32S3.spec(0)
    }
}
