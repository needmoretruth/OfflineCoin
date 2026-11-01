//! Several real nodes on one computer. Each is its own operating-system process running the same
//! node code: real hashing, real blocks and signatures, real validation, its own folder, and TCP
//! between them. What it is given is enforced by the operating system, and what it uses is
//! measured from the operating system, so the two can stand side by side.
mod child;
mod measure;
mod preset;
mod spawn;
mod supervisor;

pub use child::run_node;
pub use measure::Measured;
pub use preset::{Preset, PROCESS_OVERHEAD_MIB};
pub use spawn::Enforcement;
pub use supervisor::{address_of, ChildStatus, LocalView, Supervisor};

use serde::{Deserialize, Serialize};

/// Whose wallet a node mines to and pays from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WalletChoice {
    /// A key in the node's own folder.
    #[default]
    Own,
    /// The app's wallet, so mining adds up in one place.
    Shared,
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
}

impl Default for NodeSpec {
    fn default() -> Self {
        Preset::Esp32S3.spec(0)
    }
}
