//! One consistent picture of the app for the screen.
use super::explorer::{BlockRow, Explored, NoteRow};
use super::history::Activity;
use super::pool_run::PoolView;
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
    pub keys: Vec<KeyView>,
    pub miner: String,
    pub miner_is_chosen: bool,
    pub mine_note: String,
    pub merging: bool,
    /// The oldest block whose body is here.
    pub oldest: u64,
    pub recent: Option<u64>,
    pub fits: Option<u64>,
    /// Unspent coins the whole node tracks.
    pub unspent: usize,
    /// The peer ahead of this node whose blocks nobody can give it, and how far it is.
    pub needs_history: Option<(u32, u64)>,
    /// Set after a person chose to trust a peer's snapshot.
    pub assumed_from: Option<u32>,
    pub fetching: Option<(u32, u32)>,
    pub rebuilt: bool,
    pub blocks: Vec<BlockRow>,
    pub notes: Vec<NoteRow>,
    pub explored: Explored,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyView {
    pub address: String,
    pub public: String,
    pub receiving: bool,
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
    /// Quiet because it is restarting, not because something is wrong.
    pub returning: bool,
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
    pub pool: PoolView,
    pub notice: Option<Notice>,
    /// False when this run leaves USB alone.
    pub usb: bool,
    pub full_rate: u64,
    /// The folder this app keeps its files in.
    pub folder: String,
}
