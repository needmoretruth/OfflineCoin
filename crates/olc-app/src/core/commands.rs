//! Everything the screen can ask of the app.
use crate::local::NodeSpec;
use crate::settings::Settings;
use olc_board::Args;
use olc_control::BoardId;
use olc_core::CoinConfig;

/// Which coin the first screen settled on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartCoin {
    Default,
    New(CoinConfig),
    Join {
        coin: CoinConfig,
        address: Option<String>,
    },
}

pub enum Command {
    FinishSetup(Box<Settings>, StartCoin),
    /// Saves the settings and applies what can change at once.
    Save(Box<Settings>),
    Join {
        coin: Box<CoinConfig>,
        address: Option<String>,
    },
    SetMining(bool),
    SetCap(u64),
    Pay {
        to: String,
        amount: String,
    },
    AddAddress(String),
    Rescan,
    BoardLine {
        id: BoardId,
        line: String,
    },
    UpdateBoard(BoardId),
    StartCapTest(BoardId),
    ReadBright(BoardId),
    InstallBoard {
        path: String,
        name: String,
    },
    StartExperiment {
        boards: Vec<BoardId>,
        args: Args,
    },
    StopExperiment,
    AddNode(Box<NodeSpec>),
    RemoveNode(u32),
    StartNode(u32),
    StopNode(u32),
    NodeCap {
        id: u32,
        cap: u64,
    },
    NodeMining {
        id: u32,
        on: bool,
    },
    NodePay {
        id: u32,
        to: String,
        amount: String,
    },
}
