//! One consistent picture of the app for the screen.
use super::history::Activity;
use crate::discovery::{CoinGroup, Reach};
use crate::local::LocalView;
use olc_board::Seen;
use olc_core::{CoinConfig, Hash};

#[derive(Clone, Debug, Default)]
pub struct NodeView {
    pub coin: Option<CoinConfig>,
    pub genesis: Hash,
    pub tip: Hash,
    pub height: u64,
    pub work: u128,
    pub difficulty: u64,
    pub peers: usize,
    pub pool: usize,
    pub hashrate: u64,
    pub cap: u64,
    pub mining: bool,
    pub syncing_to: Option<u64>,
    /// Blocks read back from disk so far and in all, while the chain loads.
    pub loading: Option<(usize, usize)>,
    pub balance: u64,
    pub spendable: u64,
    pub address: String,
    pub activity: Vec<Activity>,
    pub blocks_mined: u64,
    pub port: Option<u16>,
    /// A number that goes up with every new tip, for the logo's turn.
    pub tip_changes: u64,
    pub stored_bytes: u64,
    pub storage_full: bool,
}

impl NodeView {
    pub fn tip_hex(&self) -> String {
        self.tip.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

#[derive(Clone, Debug)]
pub struct BoardView {
    pub seen: Seen,
    pub reach: Reach,
    pub job: Option<String>,
    /// The panel's stored brightness, once the board has said it.
    pub bright: Option<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct ScanView {
    pub running: bool,
    /// Addresses tried in the last look.
    pub tried: usize,
    pub subnets: Vec<String>,
    /// Seconds since the last look ended.
    pub age_secs: Option<u64>,
}

#[derive(Clone, Debug, Default)]
pub struct ExperimentView {
    pub lines: Vec<String>,
    pub running: bool,
}

/// One thing that happened, for the bar at the bottom of the window.
#[derive(Clone, Debug)]
pub struct Notice {
    pub number: u64,
    pub key: &'static str,
    pub detail: String,
    pub good: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub node: NodeView,
    pub groups: Vec<CoinGroup>,
    pub boards: Vec<BoardView>,
    pub local: Vec<LocalView>,
    pub new_ports: Vec<(String, Option<String>)>,
    pub scan: ScanView,
    pub experiment: ExperimentView,
    pub notice: Option<Notice>,
    /// False when this run leaves USB alone.
    pub usb: bool,
    pub full_rate: u64,
    /// The folder this app keeps its files in.
    pub folder: String,
}
