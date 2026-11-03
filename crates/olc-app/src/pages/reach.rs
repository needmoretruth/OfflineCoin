//! Where every word lives in the app. The tests read the words from the core and the firmware, so
//! a new word with no row here fails, and they read the pages, so a row cannot claim what its page
//! does not do.
pub struct Row {
    pub word: &'static str,
    /// The page it is on.
    pub page: &'static str,
    /// Where on that page.
    pub spot: &'static str,
    /// What the page does for this word, as its code says it: the `Command` or `BoardAction` it
    /// sends, the core function it calls, or the field it reads. Empty for no place in the app.
    pub sends: &'static str,
    /// Empty when anyone can reach it with no typing. Otherwise why only an expert needs it.
    pub expert_because: &'static str,
}

const fn row(word: &'static str, page: &'static str, spot: &'static str, sends: &'static str) -> Row {
    Row { word, page, spot, sends, expert_because: "" }
}

const fn expert(
    word: &'static str,
    page: &'static str,
    spot: &'static str,
    sends: &'static str,
    why: &'static str,
) -> Row {
    Row { word, page, spot, sends, expert_because: why }
}

const HOME: &str = "page.home";
const BOARDS: &str = "page.boards";
const COINS: &str = "page.coins";
const RECORDS: &str = "page.records";
const LOCKS: &str = "page.locks";
const SWAP: &str = "page.swap";
const SETTINGS: &str = "page.settings";

/// The words of the command line.
pub const CLI_ROWS: &[Row] = &[
    row("new-coin", COINS, "New coin", "new_coin("),
    row("keys", SETTINGS, "Wallet, addresses", "node.keys"),
    row("key", SETTINGS, "Wallet, addresses", "Personal::NewKey"),
    row("backup", SETTINGS, "Wallet, back up", "Personal::BackUp"),
    row("restore", SETTINGS, "Wallet, restore", "Personal::Restore"),
    row("miner", SETTINGS, "Mining, where coins go", "Personal::SetMiner"),
    row("keep", SETTINGS, "Keeping blocks", "Personal::SetRecent"),
    row("bluetooth", SETTINGS, "Link order, Bluetooth", "Personal::SetBluetooth"),
    expert(
        "espnow",
        SETTINGS,
        "Network, ESP-NOW card",
        "Personal::SetEspNowCard",
        "Needs a Wi-Fi card in monitor mode and its system name",
    ),
    row("merge", SETTINGS, "Keeping blocks, merge", "Personal::SetMerging"),
    expert(
        "version1",
        SETTINGS,
        "Keeping blocks, sign the old way",
        "Personal::SetVersionOne",
        "Payments a fork could copy are for people who know why they want them",
    ),
    row("light", SETTINGS, "Keeping blocks", "Personal::SetLight"),
    row("mining", SETTINGS, "Mining, start on open", "settings.mine_on_start"),
    row("watch", SETTINGS, "Wallet, watch", "Personal::Watch"),
    row("prepare", SETTINGS, "Wallet, pay from an offline computer", "Personal::Prepare"),
    row("sign", SETTINGS, "Wallet, pay from an offline computer", "Personal::Sign"),
    row("broadcast", SETTINGS, "Wallet, pay from an offline computer", "Personal::Broadcast"),
    row("note", RECORDS, "Write a note", "Personal::WriteNote"),
    row("mine-note", SETTINGS, "Mining, words in each block", "Personal::SetMineNote"),
    row("snapshot", COINS, "The card that appears when old blocks are missing", "Personal::AcceptSnapshot"),
    row("explore", RECORDS, "Search", "Personal::Explore"),
    row("mine", HOME, "Start and stop", "Command::SetMining"),
    row("cap", SETTINGS, "Mining, speed", "Command::SetCap"),
    row("pay", HOME, "Send", "Command::Pay"),
    row("add", COINS, "Add an address", "Command::AddAddress"),
    row("scan", COINS, "Look again", "Command::Rescan"),
    expert("fork", COINS, "Change rules", "Command::Fork", "Rules that part from a running chain are expert work"),
    row("fork-back", COINS, "Earlier rules", "Command::ForkBack"),
    expert("lock-preset", LOCKS, "Write a lock", "hash_lock_for(", "Beta locks are expert work"),
    expert("lock-check", LOCKS, "Write a lock", "check_lock(", "Beta locks are expert work"),
    expert("lock-pay", LOCKS, "Write a lock, pay", "LockOrder::Pay", "Beta locks are expert work"),
    expert("lock-spend", LOCKS, "Coins under a lock", "LockOrder::Spend", "Beta locks are expert work"),
    expert("lock-sign", LOCKS, "Signing together", "LockOrder::Sign", "Beta locks are expert work"),
    row("swap", SWAP, "Make an offer, take an offer", "SwapOrder::Offer"),
    row("join", COINS, "Join from a file", "Command::JoinCoinFile"),
    row("save-coin", COINS, "Save a coin file", "Command::SaveCoinFile"),
    row("boards", BOARDS, "The list", "snap.boards"),
    row("board", BOARDS, "The rows of a board", "Command::Board"),
    row("update", BOARDS, "Update", "Command::UpdateBoard"),
    row("move", BOARDS, "Move to this coin", "Command::MoveBoard"),
    expert(
        "cap-test",
        BOARDS,
        "Lab",
        "Command::StartCapTest",
        "Tuning a board's cap is measurement, and the buttons already set speed",
    ),
    row("install", BOARDS, "New board", "Command::InstallBoard"),
    row("pool-start", BOARDS, "Together, start", "Command::StartPool"),
    row("pool-stop", BOARDS, "Together, stop", "Command::StopPool"),
    expert(
        "lab-stop",
        BOARDS,
        "Lab",
        "Command::StopExperiment",
        "It stops an experiment, and experiments are expert work",
    ),
    row("node-start", BOARDS, "This computer", "Command::StartNode"),
    row("node-stop", BOARDS, "This computer", "Command::StopNode"),
    row("node-remove", BOARDS, "This computer", "Command::RemoveNode"),
    row("node-cap", BOARDS, "This computer, the node's card", "Command::NodeCap"),
    row("node-mine", BOARDS, "This computer", "Command::NodeMining"),
    row("node-miner", BOARDS, "This computer, pay", "Command::NodeMiner"),
    row("node-pay", BOARDS, "This computer, pay", "Command::NodePay"),
];

/// The words a board reads.
pub const BOARD_ROWS: &[Row] = &[
    expert("status", BOARDS, "Board, read", "BoardAction::Status", "Raw report for diagnosis; the same numbers show on the rows"),
    expert("screen", BOARDS, "Board, read", "BoardAction::ReadScreen", "Raw copy of the board's screen, for diagnosis"),
    expert("boots", BOARDS, "Board, read", "BoardAction::RecentStarts", "Start history, for diagnosis"),
    expert("logo", BOARDS, "Board, screen", "BoardAction::ShowLogo", "Plays the logo once, only useful to test the screen"),
    row("view", BOARDS, "Board, screen", "BoardAction::View"),
    row("lang", BOARDS, "Board, language", "BoardAction::Language"),
    row("mine", BOARDS, "Board, mining", "BoardAction::Mine"),
    expert("limit", BOARDS, "Board, mining", "BoardAction::Cap", "An exact cap; a person changes speed in Settings"),
    expert("smooth", BOARDS, "Board, screen", "BoardAction::Smooth", "Frame rate of the logo, a tuning number"),
    expert("power", BOARDS, "Board, radio", "BoardAction::RadioPower", "Radio power is a tuning number"),
    row("bright", BOARDS, "Board, brightness", "BoardAction::Bright"),
    expert("ignore", BOARDS, "Board, radio", "BoardAction::Ignore", "Blocking a neighbour is for radio tests"),
    row("offline", BOARDS, "Board, keep to itself", "BoardAction::Offline"),
    row("espnow", BOARDS, "Board, nearby boards", "BoardAction::EspNow"),
    row("bluetooth", BOARDS, "Board, Bluetooth", "BoardAction::Bluetooth"),
    expert("links", BOARDS, "Board, radio", "BoardAction::Links", "Link order for one board; Settings sets the order for all"),
    expert("length", BOARDS, "Terminal only", "", "It counts a cable's bytes for tools/measure, and no person asks for it"),
    expert("check-time", BOARDS, "Terminal only", "", "It times one signature check for tools/measure, and a person never needs it."),
    expert("wear", BOARDS, "Terminal only", "", "It counts flash erases for tools/measure, and a person never needs it."),
    expert("heat", BOARDS, "Terminal only", "", "It lowers the heat limit until the next start to test the guard, which acts by itself."),
    expert("near", BOARDS, "Board, read", "BoardAction::Neighbours", "A list of radio neighbours, for diagnosis"),
    expert("wires", BOARDS, "Board, radio", "BoardAction::ReadWires", "Pin swap and wire speed are for wiring tests"),
    row("pay", BOARDS, "Board, pay", "BoardAction::Pay"),
    expert("pool", BOARDS, "Board, chain", "BoardAction::ClearWaitingPayments", "Clearing waiting payments is a repair"),
    expert("tx", BOARDS, "Board, read", "BoardAction::ReadPayment", "Looking up one payment id, for diagnosis"),
    expert("trust", BOARDS, "Board, radio", "BoardAction::Trust", "Installs a control key; it needs a key in hand"),
    row("name", BOARDS, "Board, name", "BoardAction::Name"),
    expert("clock", BOARDS, "Board, mining", "BoardAction::Clock", "Chip clock is a tuning number"),
    expert("full", BOARDS, "Board, mining", "BoardAction::FullSpeed", "Top clock with no cap, which heats the board"),
    row("network", BOARDS, "Board, own Wi-Fi", "BoardAction::OwnNetwork"),
    row("wifi", BOARDS, "Board, home Wi-Fi", "BoardAction::Wifi"),
    expert("dial", BOARDS, "Board, radio", "BoardAction::Dial", "Dialing one address is a network test"),
    expert("keys", BOARDS, "Board, read", "BoardAction::ReadKeys", "Raw key list, for diagnosis"),
    expert("key", BOARDS, "Board, chain", "BoardAction::NewKey", "A board's keys are for experts; the app's wallet is the person's"),
    row("mineto", BOARDS, "Board, coins go to", "BoardAction::MineTo"),
    expert("keep", BOARDS, "Board, chain", "BoardAction::Keep", "Block keeping on a board is a storage tuning"),
    expert("light", BOARDS, "Board, chain", "BoardAction::Light", "Headers only on a board is a storage tuning"),
    expert("merge", BOARDS, "Board, chain", "BoardAction::Merge", "Merging on a board is a storage tuning"),
    expert("note", BOARDS, "Board, chain", "BoardAction::Note", "Notes go through the app in Records"),
    expert("minenote", BOARDS, "Board, chain", "BoardAction::MineNote", "Words in a board's blocks; Settings sets them for the app"),
    expert("snapshot", BOARDS, "Board, chain", "BoardAction::AcceptSnapshot", "Only a board stuck without old blocks needs it"),
    row("coin", BOARDS, "Board, move to this coin", "Command::MoveBoard"),
    expert("work", BOARDS, "Board, chain", "BoardAction::StopWork", "The pool sends it by itself, and the stop word is a repair"),
];

#[cfg(test)]
#[path = "reach_tests.rs"]
mod tests;
