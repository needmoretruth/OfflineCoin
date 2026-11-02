//! Everything the screen can ask of the app.
use super::board_action::BoardAction;
use crate::local::NodeSpec;
use crate::settings::Settings;
use olc_board::Args;
use olc_control::BoardId;
use olc_core::CoinConfig;
use olc_node::Rank;
use olc_store::coin_file::NewCoin;

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
    ListKeys,
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
    /// Whether this node holds headers only.
    SetLight(bool),
    /// Whether this node mines from its next start.
    SetMining(bool),
    /// A payment from the watched addresses, for a wallet elsewhere to sign.
    Prepare {
        to: String,
        units: u64,
        fee: u64,
        /// Where to write it. Without it, the answer is the payment itself.
        save: Option<String>,
    },
    /// Signs the payment in this file.
    Sign {
        path: String,
        /// Where to write the signed payment. Without it, the answer is the payment itself.
        save: Option<String>,
    },
    /// Sends the signed payment in this file.
    Broadcast(String),
    SetLinks(Rank),
    /// Reaches neighbours by Bluetooth too.
    SetBluetooth(bool),
    /// A Wi-Fi card for ESP-NOW. Empty means none.
    SetEspNowCard(String),
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
    /// The order links are preferred in, here and on the boards.
    SetLinks(Rank),
    Rescan,
    OpenDataFolder,
    Board {
        id: BoardId,
        action: BoardAction,
    },
    /// Writes a new coin to a file, for this node or someone else's to start from.
    NewCoinFile {
        choices: NewCoin,
        path: String,
    },
    /// New rules on the running coin, keeping its genesis and history. `at` is the height they
    /// start from, and without it the next block. `changes` are coin field keys and their text.
    Fork {
        at: Option<u64>,
        changes: Vec<(String, String)>,
    },
    /// Back to the rules before the last fork.
    ForkBack,
    /// Joins the coin a file describes.
    JoinCoinFile(String),
    /// Writes the running coin to a file someone else can join from.
    SaveCoinFile(String),
    UpdateBoard(BoardId),
    /// Puts a board on the coin this app runs. The person already said yes.
    MoveBoard(BoardId),
    StartCapTest(BoardId),
    ReadBoardState(BoardId),
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
    Lock(LockOrder),
}

/// Something to do with coins under a beta lock.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LockOrder {
    /// Puts coins from this wallet under the lock these words make.
    Pay { words: String, amount: String },
    /// Spends the coin named `coin` with these unlock words. An empty `to` is this wallet.
    Spend { coin: String, to: String, unlock: String },
    /// Adds this wallet's signatures to a spend another signer passed on.
    Sign { text: String },
}
