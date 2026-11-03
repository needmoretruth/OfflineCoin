//! One consistent picture of the app for the screen.
use olc_node::locks::LockedCoin;
use olc_node::{Keeps, LightStatus};
use super::explorer::{BlockRow, Explored, NoteRow};
use super::history::Activity;
use super::pool_run::PoolView;
use super::radios::NoBluetooth;
use super::swap::Stage;
use crate::discovery::{CoinGroup, Reach};
use crate::local::LocalView;
use olc_board::{BoardState, Seen};
use olc_core::{CoinConfig, Hash};
use olc_node::Rank;

#[derive(Clone, Debug, Default)]
pub struct NodeView {
    pub coin: Option<CoinConfig>,
    pub genesis: Hash,
    pub tip: Hash,
    pub height: u64,
    pub work: u128,
    pub difficulty: u64,
    pub keeps: Keeps,
    pub peers: usize,
    pub pool: usize,
    pub hashrate: u64,
    pub cap: u64,
    pub mining: bool,
    /// How a node that holds headers only stands. `None` on a node that holds blocks.
    pub light: Option<LightStatus>,
    /// Addresses followed without a key, with their coins.
    pub watched: Vec<(String, u64)>,
    pub syncing_to: Option<u64>,
    /// The chain is being read back from disk, so the rest of this is from before.
    pub loading: bool,
    pub balance: u64,
    pub spendable: u64,
    /// Mined coins still too young to spend, which the balance leaves out.
    pub maturing: u64,
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
    /// Payments are signed as version 1.
    pub version_one: bool,
    pub links: Rank,
    /// Whether this node uses Bluetooth. `None` where it cannot.
    pub bluetooth: Option<bool>,
    pub no_bluetooth: Option<NoBluetooth>,
    /// The oldest block whose body is here.
    pub oldest: u64,
    pub recent: Option<u64>,
    pub fits: Option<u64>,
    /// Unspent coins the whole node tracks.
    pub unspent: usize,
    /// The peer ahead of this node whose blocks nobody can give it, and the oldest block one holds.
    pub needs_history: Option<(u32, u64)>,
    /// Set after a person chose to trust a peer's snapshot.
    pub assumed_from: Option<u32>,
    pub fetching: Option<(u32, u32)>,
    pub rebuilt: bool,
    pub blocks: Vec<BlockRow>,
    pub notes: Vec<NoteRow>,
    pub explored: Explored,
    /// Coins under a beta or gamma lock.
    pub locks: Vec<LockedCoin>,
    /// A spend waiting for another signer, and whose signatures it needs.
    pub half_spend: Option<(String, Vec<String>)>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyView {
    pub address: String,
    pub public: String,
    pub receiving: bool,
}

impl NodeView {
    /// About how long this computer takes to find a block alone. `None` when it is not trying.
    pub fn seconds_per_block(&self) -> Option<u64> {
        (self.hashrate > 0).then(|| (self.difficulty / self.hashrate.max(1)).max(1))
    }

    pub fn tip_hex(&self) -> String {
        self.tip.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

#[derive(Clone, Debug)]
pub struct BoardView {
    pub seen: Seen,
    pub reach: Reach,
    pub job: Option<String>,
    /// The last answer to something a person asked of this board.
    pub answer: Option<String>,
    /// What it holds of its chain, once it has said.
    pub keeps: Keeps,
    /// Quiet because it is restarting, not because something is wrong.
    pub returning: bool,
    /// Its settings, once the board has said them.
    pub state: BoardState,
    /// On the coin this app runs, by other rules.
    pub other_rules: bool,
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

/// One swap as a screen shows it. `give` and `get` are a ticker and units.
#[derive(Clone, Debug)]
pub struct SwapView {
    pub id: String,
    pub maker: bool,
    pub stage: Stage,
    pub give: (String, u64),
    pub get: (String, u64),
    /// The text to hand to the other side, while it still needs it.
    pub text: Option<String>,
    /// The block from which this side's coins come back, on the coin it pays with.
    pub refund_at: Option<u64>,
    /// The height of the coin this side pays with.
    pub height: u64,
    pub wait_minutes: u32,
}

#[derive(Clone, Debug, Default)]
pub struct SwapsView {
    pub swaps: Vec<SwapView>,
    /// The node on the other coin of an open swap: its ticker, height and peers.
    pub other: Option<(String, u64, usize)>,
    /// Coins this app ran besides the one it runs now: folder name and ticker.
    pub coins: Vec<(String, String)>,
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
    pub swaps: SwapsView,
}
