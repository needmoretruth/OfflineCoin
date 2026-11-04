//! Everything the screen can ask of the app.
use super::board_action::BoardAction;
use crate::local::NodeSpec;
use crate::settings::Settings;
use olc_board::Args;
use olc_control::BoardId;
use olc_core::CoinConfig;
use olc_node::Rank;
use olc_store::coin_file::NewCoin;
use std::sync::mpsc::Sender;

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
    /// Whether this node signs its payments as version 1.
    SetVersionOne(bool),
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
    /// Seed words, signed messages and shared wallets.
    Wallet(super::wallet_orders::WalletOrder),
}

impl Personal {
    /// Whether this order needs a node running. The others also work on a node's folder.
    pub(super) fn needs_a_node(&self) -> bool {
        matches!(self, Self::Prepare { .. } | Self::Broadcast(_) | Self::WriteNote(_))
    }

    /// Whether this order is only done once it has left this computer.
    pub(super) fn goes_out(&self) -> bool {
        matches!(self, Self::Broadcast(_) | Self::WriteNote(_))
    }
}

// A command lives for one trip through the channel, so its size costs nothing a box would save:
// the setup's coin carries its difficulty and time rules.
#[allow(clippy::large_enum_variant)]
pub enum Command {
    /// Closes the app. The answer comes once nothing a person asked for is still waiting.
    Finish(Sender<()>),
    Own(Personal),
    FinishSetup(Box<Settings>, StartCoin),
    /// Saves the settings and applies what can change at once.
    Save(Box<Settings>),
    /// Home's line that OLC started over was read.
    CloseOldOlcNote,
    Join {
        coin: Box<CoinConfig>,
        address: Option<String>,
    },
    SetMining(bool),
    SetCap(u64),
    /// The Lab's miners that hash now, by their number in the Lab, and the speed each was set to:
    /// they share the one limit with this app and its miners.
    LabMiners(Vec<(u32, u64)>),
    Pay {
        to: String,
        amount: String,
    },
    AddAddress(String),
    /// The order links are preferred in, here and on the boards.
    SetLinks(Rank),
    Rescan,
    OpenDataFolder,
    /// Opens the file the settings live in, as Bitcoin Core's options open its configuration.
    OpenSettingsFile,
    /// Settings' Reset to defaults, for the node: what it keeps, where mined coins go, the note in
    /// its blocks, the links' order, Bluetooth and the ESP-NOW card go back to how they came.
    ResetNodePreferences,
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
    Pool(super::pool_plan::PoolOrder),
    /// The waiting payments, this wallet's coins, or blocks made now.
    Chain(super::making::ChainOrder),
    NodePay {
        id: u32,
        to: String,
        amount: String,
    },
    Lock(LockOrder),
    Channel(ChannelOrder),
    Swap(SwapOrder),
}

impl Command {
    /// Whether this acts on the running node or the coin it runs. Such an order waits while a
    /// chain is on its way in, because the engine there then is thrown away.
    pub(super) fn needs_the_chain(&self) -> bool {
        matches!(
            self,
            Self::Own(_)
                | Self::FinishSetup(..)
                | Self::Join { .. }
                | Self::JoinCoinFile(_)
                | Self::Pay { .. }
                | Self::SetLinks(_)
                | Self::Fork { .. }
                | Self::ForkBack
                | Self::MoveBoard(_)
                | Self::Pool(_)
                | Self::Chain(_)
                | Self::Lock(_)
                | Self::Channel(_)
                | Self::Swap(_)
        )
    }
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
    /// Pays from shared wallet number `wallet`, signed here and passed on for the rest.
    PayShared { wallet: usize, to: String, amount: String },
    /// Speeds up the waiting payment with this id: replaced when sent from here, pulled along by
    /// a child when it pays here.
    SpeedUp { id: String },
    /// Moves the coins on keys from before the seed words to the words, so the words alone
    /// bring every coin back.
    MoveToWords,
}

/// Something to do with payment channels. Amounts are typed as people type them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChannelOrder {
    /// Everything about the channels, as text.
    Show,
    /// Opens a channel to a node: one heard lately, named by its name or the start of its key, or
    /// any node by `key@host:port`, which is dialled first. An empty `delay` is the usual wait and
    /// an empty `push` gives the other side nothing.
    Open { peer: String, amount: String, delay: String, push: String },
    /// Closes together with the other side, or alone with `force`.
    Close { channel: String, force: bool },
    /// Makes a payment request.
    Invoice { amount: String, note: String },
    /// Pays a request through the channels.
    Pay { text: String },
    /// Reads a request and finds who it pays and the fee, before anyone presses pay. Says nothing.
    Quote { text: String },
    /// Every connection of the node, as text, each with the number that drops it.
    Peers,
    /// Ends one connection on the network link, by its number there.
    Disconnect(u32),
    /// Keeps an address off the network link for `hours`, and ends its connections now.
    Ban { address: String, hours: u64 },
    Unban(String),
}

/// Something to do with a coin swap.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SwapOrder {
    /// Offers `give` of the coin this node runs for `take` of `coin`: a coin this app ran before,
    /// by its folder name, or a coin file. Each side may take its coins back after `wait_minutes`.
    Offer { coin: String, give: String, take: String, wait_minutes: u32 },
    /// Takes the offer this text holds.
    Accept(String),
    /// Starts an offer with the answer this text holds.
    Answer(String),
    /// Ends the swap whose id starts so, before any coin is locked.
    Cancel(String),
    List,
}
