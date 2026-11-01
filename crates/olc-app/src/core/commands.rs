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

/// What a person changes on this app's own node.
pub enum Personal {
    NewKey,
    ImportKey(String),
    UseKey(usize),
    ExportKey {
        index: usize,
        path: String,
    },
    BackUp {
        path: String,
        password: Option<String>,
    },
    Restore {
        path: String,
        password: Option<String>,
    },
    /// Empty goes back to this node's own key.
    SetMiner(String),
    SetMineNote(String),
    SetRecent(Option<u64>),
    SetMerging(bool),
    Watch(String),
    WriteNote(String),
    AcceptSnapshot,
    Explore(String),
}

pub enum Command {
    Own(Personal),
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
    /// Puts a board on the coin this app runs. The person already said yes.
    MoveBoard(BoardId),
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
    /// Whether this app's own node holds headers only.
    SetLight(bool),
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
    NodeMiner {
        id: u32,
        to: String,
    },
    StartPool {
        share_difficulty: u64,
        nodes: Vec<u32>,
        boards: Vec<BoardId>,
    },
    StopPool,
    NodePay {
        id: u32,
        to: String,
        amount: String,
    },
}
