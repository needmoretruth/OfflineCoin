//! The Lab on this computer: its own coin, kept apart from the person's chain and wallet, and
//! what its experiments show. The network of participants lives in `lab_net`; the double spend
//! and the split of olc-lab run on its nodes through the lines a board answers, so a
//! reorganisation here replaces lab blocks only, and nobody else's node is touched.
use olc_core::coin::COIN;
use olc_core::{CoinConfig, Rules};
use olc_lab::{Outcome, Step};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

pub use super::lab_picture::{Frame, Mark, Picture, Row, Side};

/// Expected hashes for one block. Three miners at their usual speed find one about every two
/// seconds, slow enough to watch and quick enough to end an experiment in a minute or two.
const DIFFICULTY: u64 = 48_000;
/// The most blocks one press makes, as the console's `generate` allows.
pub const MOST_BLOCKS: u64 = 1_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabKind {
    Attack,
    Split,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Idle,
    /// Miners are starting.
    Starting,
    /// The attacker mines coins it can spend.
    Warming,
    Running,
    Done,
    Failed,
}

#[derive(Clone, Debug, Default)]
pub struct LabState {
    pub phase: Phase,
    pub kind: Option<LabKind>,
    pub frames: Vec<Frame>,
    pub live: Picture,
    pub lines: Vec<String>,
    pub outcome: Option<Outcome>,
    pub error: Option<String>,
}

impl LabState {
    /// The newest step, for a test that waits on one.
    pub fn reached(&self, step: impl Fn(&Step) -> bool) -> bool {
        self.frames.iter().any(|frame| step(&frame.step))
    }
}

/// The program and folder for lab nodes. The app's own program and data folder unless a test
/// names others.
static PLACE: OnceLock<Mutex<Option<(PathBuf, PathBuf)>>> = OnceLock::new();

/// For runs outside the installed app, such as tests: the node program and a folder of their own.
pub fn use_place(program: PathBuf, folder: PathBuf) {
    if let Ok(mut place) = PLACE.get_or_init(Mutex::default).lock() {
        *place = Some((program, folder));
    }
}

pub(crate) fn place() -> Result<(PathBuf, PathBuf), String> {
    let chosen = PLACE.get_or_init(Mutex::default).lock().ok().and_then(|place| place.clone());
    match chosen {
        Some(place) => Ok(place),
        None => {
            let program = std::env::current_exe().map_err(|error| error.to_string())?;
            Ok((program, crate::settings::standard_folder()?.join("lab")))
        }
    }
}

/// A stopped node may write for a moment longer, so its folder goes once it surely ended.
pub(crate) fn remove_later(folder: PathBuf) {
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(3));
        let _ = std::fs::remove_dir_all(folder);
    });
}

pub(crate) fn lab_coin() -> CoinConfig {
    CoinConfig {
        name: "OLC Lab".into(),
        ticker: "LAB".into(),
        message: "OLC Lab, this computer only".into(),
        genesis_time_ms: crate::core::unix_ms(),
        initial_difficulty: DIFFICULTY,
        premine: Vec::new(),
        rules: Rules {
            block_time_ms: 2_000,
            retarget_interval: 0,
            reward: 50 * COIN,
            coinbase_maturity: 2,
            min_difficulty: DIFFICULTY,
            floor_from_height: 0,
            ..Rules::default()
        },
        upgrade_from: 0,
        ..CoinConfig::olc()
    }
}
