//! Every action by name. The command line and a typed console read words here, buttons send the
//! same [`Command`] directly, and nothing behind it knows which of them asked.
use super::board_action::BoardAction;
use super::commands::{Command, LockOrder, Personal};
use super::host_locks::{check_lock, hash_lock_for, time_lock_for, two_of_two_for};
use super::snapshot::BoardView;
use super::words_chain as chain;
use super::words_channels as ch;
use super::words_pool as pool;
use super::words_swap::swap;
use super::words_wallet;
use olc_board::Args;
use olc_control::line::split;
use olc_control::BoardId;
use olc_lang::words::parse_words;
use olc_store::coin_fields::{new_coin, CoinField, NewCoin, COIN_FIELDS};

/// Where actions land: the app's running core, or a terminal.
pub trait Desk {
    /// Carries out `command` and gives what to show for it.
    fn command(&mut self, command: Command) -> Result<String, String>;
    fn boards(&mut self) -> Result<Vec<BoardView>, String>;
    /// Every miner that could join a pool, boards heard only by radio included. A desk with no
    /// running core knows none.
    fn choices(&mut self) -> Vec<super::Choice> {
        Vec::new()
    }
    /// A password asked for out of sight, so it stays off every command line.
    fn password(&mut self) -> Result<String, String>;
}

type Run = fn(&mut dyn Desk, &Args) -> Result<String, String>;

/// One action.
pub struct Word {
    pub name: &'static str,
    pub usage: &'static str,
    pub run: Run,
}

pub const WORDS: &[Word] = &[
    Word { name: "keys", usage: "keys                 list keys", run: keys },
    Word { name: "key", usage: "key new|use N|import HEX|export N FILE", run: key },
    Word { name: "backup", usage: "backup FILE [--password]", run: backup },
    Word { name: "restore", usage: "restore FILE [--password]", run: restore },
    Word { name: "miner", usage: "miner ADDRESS|off    where mined coins go", run: miner },
    Word { name: "keep", usage: "keep BLOCKS|all      recent blocks to hold", run: keep },
    Word { name: "merge", usage: "merge on|off         merge small coins by itself", run: merge },
    Word { name: "version1", usage: "version1 on|off      sign payments as the first Bitcoin did", run: version_one },
    Word { name: "bluetooth", usage: "bluetooth on|off     reach neighbours by Bluetooth too", run: bluetooth },
    Word { name: "espnow", usage: "espnow CARD|off      a Wi-Fi card in monitor mode for ESP-NOW", run: esp_now },
    Word { name: "light", usage: "light on|off         hold headers only, trusting peers on unspent coins", run: light },
    Word { name: "mining", usage: "mining on|off        mine from the start", run: mining },
    Word { name: "watch", usage: "watch ADDRESS        follow an address without its key", run: watch },
    Word { name: "prepare", usage: "prepare ADDRESS UNITS [--fee UNITS]  a payment from watched addresses", run: prepare },
    Word { name: "sign", usage: "sign FILE            sign that payment, offline", run: sign },
    Word { name: "broadcast", usage: "broadcast FILE       send it", run: broadcast },
    Word { name: "note", usage: "note TEXT            write a note into a block", run: note },
    Word { name: "mine-note", usage: "mine-note TEXT       a note in every mined block", run: mine_note },
    Word { name: "snapshot", usage: "snapshot accept      take a peer's unspent set", run: snapshot },
    Word { name: "explore", usage: "explore TEXT         find a block, payment or address", run: explore },
    Word { name: "mine", usage: "mine on|off", run: mine },
    Word { name: "cap", usage: "cap HASHES           hashes per second here, 0 for no cap", run: cap },
    Word { name: "pay", usage: "pay ADDRESS AMOUNT", run: pay },
    Word { name: "add", usage: "add ADDRESS          a node to look for", run: add },
    Word { name: "scan", usage: "scan                 look for nodes now", run: scan },
    Word {
        name: "new-coin",
        usage: "new-coin FILE [--name NAME] [--ticker TICKER] [--message TEXT] [--message-in-block on|off]\n              \
                [--genesis-time-ms MS] [--difficulty N] [--premine \"ADDRESS AMOUNT; ...\"]\n              \
                [--block-time-ms MS] [--retarget BLOCKS] [--reward COINS] [--halving BLOCKS]\n              \
                [--maturity BLOCKS] [--max-block-bytes BYTES] [--min-fee COINS]\n              \
                [--min-difficulty N] [--floor-from-height HEIGHT] [--difficulty-rule bitcoin|bip94|asert]\n              \
                [--half-life BLOCKS] [--min-difficulty-blocks on|off] [--median-time BLOCKS]\n              \
                [--max-ahead-ms MS] [--bitcoin-header on|off]",
        run: make_coin,
    },
    Word {
        name: "fork",
        usage: "fork [--at HEIGHT] [--block-time-ms MS] [--retarget BLOCKS] [--reward COINS] [--halving BLOCKS]\n              \
                [--maturity BLOCKS] [--max-block-bytes BYTES] [--min-fee COINS]\n              \
                [--min-difficulty N] [--floor-from-height HEIGHT] [--difficulty-rule bitcoin|bip94|asert]\n              \
                [--half-life BLOCKS] [--min-difficulty-blocks on|off] [--median-time BLOCKS]\n              \
                [--max-ahead-ms MS]   new rules on this same chain",
        run: fork,
    },
    Word { name: "fork-back", usage: "fork-back            go back to the rules before the last fork", run: fork_back },
    Word { name: "lock-preset", usage: "lock-preset hash SECRET|two-of-two KEY KEY|time HEIGHT ADDRESS", run: lock_preset },
    Word { name: "lock-check", usage: "lock-check WORDS     check a lock and say what it costs", run: lock_check },
    Word { name: "lock-pay", usage: "lock-pay AMOUNT WORDS  put coins under a lock", run: lock_pay },
    Word { name: "lock-spend", usage: "lock-spend COIN [--to ADDRESS] WORDS  spend a locked coin", run: lock_spend },
    Word { name: "lock-sign", usage: "lock-sign TEXT       add your signature to a spend", run: lock_sign },
    Word { name: "channels", usage: "channels             your channels, channel payments and requests", run: ch::channels },
    Word { name: "channel-open", usage: "channel-open KEY@HOST:PORT AMOUNT [--push AMOUNT] [--delay BLOCKS]  connect and open a channel in one step", run: ch::channel_open },
    Word { name: "channel-close", usage: "channel-close CHANNEL [--force]  close together, or alone and wait", run: ch::channel_close },
    Word { name: "invoice", usage: "invoice AMOUNT [NOTE]  a request to be paid through channels", run: ch::invoice },
    Word { name: "channel-pay", usage: "channel-pay REQUEST  pay a request through channels", run: ch::channel_pay },
    Word { name: "peers", usage: "peers                every connection, numbered", run: ch::peers },
    Word { name: "disconnect", usage: "disconnect NUMBER    end one connection", run: ch::disconnect },
    Word { name: "ban", usage: "ban ADDRESS [1h|1d]  keep an address out for an hour or a day", run: ch::ban },
    Word { name: "unban", usage: "unban ADDRESS        let a banned address back", run: ch::unban },
    Word { name: "mempool", usage: "mempool              payments waiting for a block, as getrawmempool", run: chain::mempool },
    Word { name: "coins", usage: "coins                this wallet's coins and their confirmations, as listunspent", run: chain::coins },
    Word { name: "generate", usage: "generate N|stop      mine N blocks now at full speed, as generatetoaddress", run: chain::generate },
    Word { name: "swap", usage: "swap offer COIN GIVE TAKE [--wait MINUTES] | accept TEXT | answer TEXT | cancel ID | list", run: swap },
    Word { name: "words", usage: "words [written|new [korean]|restore WORD...|move]  seed words to write down, restore from, or move older coins to", run: words_wallet::words },
    Word { name: "sign-message", usage: "sign-message [--address ADDRESS] TEXT  sign a message with a key", run: words_wallet::sign_message },
    Word { name: "verify-message", usage: "verify-message ADDRESS SIGNATURE TEXT  check a signed message", run: words_wallet::verify_message },
    Word { name: "shared", usage: "shared [new N KEY...|add TEXT|forget N|pay N ADDRESS AMOUNT]  wallets several people share", run: words_wallet::shared },
    Word { name: "speed-up", usage: "speed-up ID          a waiting payment, with a higher fee", run: words_wallet::speed_up },
    Word { name: "wallet", usage: "wallet [encrypt|unlock [SECONDS]|lock|password]  lock the wallet with a password", run: super::words_lock::wallet },
    Word { name: "contacts", usage: "contacts             other people's addresses by name", run: super::words_lock::contacts },
    Word { name: "contact", usage: "contact add NAME ADDRESS|rename NAME NEW|remove NAME", run: super::words_lock::contact },
    Word { name: "join", usage: "join FILE            join the coin in a file", run: join },
    Word { name: "save-coin", usage: "save-coin FILE       write this coin to a file", run: save_coin },
    Word { name: "boards", usage: "boards               list boards", run: boards },
    Word { name: "board", usage: "board BOARD LINE     tell a board, by name or id", run: board },
    Word { name: "update", usage: "update BOARD         put this build on a board", run: update },
    Word { name: "move", usage: "move BOARD           put a board on this coin", run: move_board },
    Word { name: "cap-test", usage: "cap-test BOARD       find a board's best cap", run: cap_test },
    Word { name: "install", usage: "install --port PORT --name NAME [--firmware FILE]", run: install },
    Word { name: "pool", usage: "pool                 how the pools stand", run: pool::pool },
    Word {
        name: "pool-start",
        usage: "pool-start [--port 3333] [--open] [--pplns N] [--fee PERCENT] [--difficulty D] [--workers own,1,BOARD]",
        run: pool::pool_start,
    },
    Word { name: "pool-stop", usage: "pool-stop", run: pool::pool_stop },
    Word { name: "pool-join", usage: "pool-join own|NODE|BOARD HOST:PORT ADDRESS.WORKER", run: pool::pool_join },
    Word { name: "pool-leave", usage: "pool-leave own|NODE|BOARD", run: pool::pool_leave },
    Word { name: "lab-stop", usage: "lab-stop             stop an experiment", run: lab_stop },
    Word { name: "node-start", usage: "node-start ID        start a node on this computer", run: node_start },
    Word { name: "node-stop", usage: "node-stop ID", run: node_stop },
    Word { name: "node-remove", usage: "node-remove ID", run: node_remove },
    Word { name: "node-cap", usage: "node-cap ID HASHES", run: node_cap },
    Word { name: "node-mine", usage: "node-mine ID on|off", run: node_mine },
    Word { name: "node-miner", usage: "node-miner ID ADDRESS", run: node_miner },
    Word { name: "node-pay", usage: "node-pay ID ADDRESS AMOUNT", run: node_pay },
];

/// Runs the action `args` names, or gives `None` when no action has that name.
pub fn run_words(desk: &mut dyn Desk, args: &Args) -> Option<Result<String, String>> {
    let name = args.command()?;
    WORDS.iter().find(|word| word.name == name).map(|word| (word.run)(desk, args))
}

/// Runs a typed line, for a console. Double quotes hold spaces, as on a shell's command line.
pub fn run_line(desk: &mut dyn Desk, line: &str) -> Result<String, String> {
    let args = Args::parse(split(line).into_iter());
    run_words(desk, &args).unwrap_or_else(|| Err(format!("No action called {line}")))
}

/// Why a board cannot take a value, in English for the command line.
fn fault_text(fault: &str) -> &'static str {
    match fault {
        "notice.name_too_long" => "That name is too long. A board keeps 32 bytes, about 10 Korean letters.",
        "notice.wifi_name_too_long" => "A Wi-Fi name has 32 bytes at most.",
        "notice.wifi_password_bad" => "A Wi-Fi password has 8 to 63 bytes.",
        _ => "A name cannot be empty or span lines.",
    }
}

/// Words of a script from the command line. A shell takes quotes off, so an argument that reads
/// as no script is text and gets them back.
fn script(args: &Args, from: usize) -> String {
    let words = (from..).map_while(|index| args.word(index)).map(|word| match parse_words(word) {
        Ok(_) => word.to_string(),
        Err(_) => format!("\"{}\"", word.replace('\\', "\\\\").replace('"', "\\\"")),
    });
    words.collect::<Vec<_>>().join(" ")
}

pub fn usage() -> String {
    WORDS.iter().map(|word| format!("  olc {}\n", word.usage)).collect()
}

/// Everything from word `from` on, for text with spaces in it.
fn rest(args: &Args, from: usize) -> String {
    (from..).map_while(|index| args.word(index)).collect::<Vec<_>>().join(" ")
}

fn first<'a>(args: &'a Args, saying: &str) -> Result<&'a str, String> {
    args.word(1).ok_or_else(|| saying.to_string())
}

fn switch(args: &Args) -> Result<bool, String> {
    match args.word(1) {
        Some("on") => Ok(true),
        Some("off") => Ok(false),
        _ => Err("Say on or off.".into()),
    }
}

fn number<T: std::str::FromStr>(word: Option<&str>) -> Result<T, String> {
    word.and_then(|word| word.parse().ok()).ok_or_else(|| "Give a number.".to_string())
}

fn own(desk: &mut dyn Desk, order: Personal) -> Result<String, String> {
    desk.command(Command::Own(order))
}

fn keys(desk: &mut dyn Desk, _args: &Args) -> Result<String, String> {
    own(desk, Personal::ListKeys)
}

fn key(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let order = match args.word(1) {
        Some("new") => Personal::NewKey,
        Some("use") => Personal::UseKey(number(args.word(2))?),
        Some("import") => {
            Personal::ImportKey(args.word(2).ok_or("That is not a secret key.")?.into())
        }
        Some("export") => Personal::ExportKey {
            index: number(args.word(2))?,
            path: args.word(3).ok_or("Name the file: olc key export 1 key.txt")?.into(),
        },
        _ => return Err("Use: olc key new | use N | import HEX | export N FILE".into()),
    };
    own(desk, order)
}

fn file_and_password(
    desk: &mut dyn Desk,
    args: &Args,
) -> Result<(String, Option<String>), String> {
    let path = first(args, "Name the file.")?.to_string();
    let password = if args.has("password") { Some(desk.password()?) } else { None };
    Ok((path, password))
}

fn backup(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let (path, password) = file_and_password(desk, args)?;
    own(desk, Personal::BackUp { path, password })
}

fn restore(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let (path, password) = file_and_password(desk, args)?;
    own(desk, Personal::Restore { path, password })
}

fn miner(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let target = match first(args, "Say an address, or off.")? {
        "off" => "",
        address => address,
    };
    own(desk, Personal::SetMiner(target.into()))
}

fn keep(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let recent = match first(args, "Say how many blocks, or all.")? {
        "all" => None,
        blocks => Some(blocks.parse().map_err(|_| "Say how many blocks, or all.")?),
    };
    own(desk, Personal::SetRecent(recent))
}

fn bluetooth(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let on = switch(args)?;
    own(desk, Personal::SetBluetooth(on))
}

fn esp_now(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let card = first(args, "Name the card, or off.")?;
    own(desk, Personal::SetEspNowCard(if card == "off" { String::new() } else { card.into() }))
}

fn merge(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let on = switch(args)?;
    own(desk, Personal::SetMerging(on))
}

fn version_one(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let on = switch(args)?;
    own(desk, Personal::SetVersionOne(on))
}

fn note(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    first(args, "Write the note: olc note \"hello\"")?;
    own(desk, Personal::WriteNote(rest(args, 1)))
}

fn mine_note(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    own(desk, Personal::SetMineNote(rest(args, 1)))
}

fn snapshot(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    match args.word(1) {
        Some("accept") => own(desk, Personal::AcceptSnapshot),
        _ => Err("Use: olc snapshot accept".into()),
    }
}

fn explore(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    first(args, "Say what to find: a height, a block or payment id, or an address.")?;
    own(desk, Personal::Explore(rest(args, 1)))
}

fn light(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    own(desk, Personal::SetLight(switch(args)?))
}

fn mining(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    own(desk, Personal::SetMining(switch(args)?))
}

fn watch(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let address = first(args, "Name the address: olc watch ADDRESS")?;
    own(desk, Personal::Watch(address.to_string()))
}

fn prepare(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let to = first(args, "Name the address: olc prepare ADDRESS UNITS")?.to_string();
    let units = number(args.word(2))?;
    let fee = args.value("fee").map_or(Ok(0), |word| number(Some(word)))?;
    own(desk, Personal::Prepare { to, units, fee, save: None })
}

fn sign(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let path = first(args, "Name the file: olc sign unsigned.json")?;
    own(desk, Personal::Sign { path: path.to_string(), save: None })
}

fn broadcast(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let path = first(args, "Name the file: olc broadcast signed.json")?;
    own(desk, Personal::Broadcast(path.to_string()))
}

fn mine(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let on = switch(args)?;
    desk.command(Command::SetMining(on))
}

fn cap(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    desk.command(Command::SetCap(number(args.word(1))?))
}

fn pay(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let to = first(args, "Use: olc pay ADDRESS AMOUNT")?.to_string();
    let amount = args.word(2).ok_or("Say how much.")?.to_string();
    desk.command(Command::Pay { to, amount })
}

fn add(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    desk.command(Command::AddAddress(first(args, "Say the address.")?.into()))
}

fn scan(desk: &mut dyn Desk, _args: &Args) -> Result<String, String> {
    desk.command(Command::Rescan)
}

/// The `--flags` of the coin fields `pick` keeps, as field keys and their text.
fn coin_values(args: &Args, pick: fn(&CoinField) -> bool) -> Vec<(String, String)> {
    let fields = COIN_FIELDS.iter().filter(|field| pick(field));
    let given = fields.flat_map(|field| {
        args.values(field.flag).into_iter().map(|text| (field.key.to_string(), text))
    });
    given.collect()
}

fn make_coin(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let choices = NewCoin { values: coin_values(args, |_| true) };
    new_coin(&choices).map_err(|bad| bad.to_string())?;
    let path = first(args, "Name the file: olc new-coin my-coin.json")?.to_string();
    desk.command(Command::NewCoinFile { choices, path })
}

fn fork(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let at = args.value("at").map(|word| number(Some(word))).transpose()?;
    let changes = coin_values(args, CoinField::is_rule);
    if changes.is_empty() {
        return Err("Say which rule changes, like olc fork --reward 25".into());
    }
    desk.command(Command::Fork { at, changes })
}

fn fork_back(desk: &mut dyn Desk, _args: &Args) -> Result<String, String> {
    desk.command(Command::ForkBack)
}

fn lock_preset(_desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let words = match (args.word(1), args.word(2), args.word(3)) {
        (Some("hash"), Some(_), _) => Some(hash_lock_for(&rest(args, 2))),
        (Some("two-of-two"), Some(first), Some(second)) => two_of_two_for(first, second),
        (Some("time"), Some(height), Some(address)) => time_lock_for(height, address),
        _ => return Err("Use: olc lock-preset hash SECRET | two-of-two KEY KEY | time HEIGHT ADDRESS".into()),
    };
    words.ok_or_else(|| "A key or an address does not read right.".to_string())
}

fn lock_check(_desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let cost = check_lock(&script(args, 1)).map_err(|error| error.to_string())?;
    Ok(format!("{} bytes, {} words, {} signature checks", cost.bytes, cost.ops, cost.signature_checks))
}

fn lock_pay(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let amount = first(args, "Say how much, then the words of the lock.")?.to_string();
    desk.command(Command::Lock(LockOrder::Pay { words: script(args, 2), amount }))
}

fn lock_spend(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let coin = first(args, "Name the coin, as in olc locks.")?.to_string();
    let to = args.value("to").unwrap_or_default().to_string();
    desk.command(Command::Lock(LockOrder::Spend { coin, to, unlock: script(args, 2) }))
}

fn lock_sign(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    first(args, "Paste the spend someone passed you.")?;
    let text = rest(args, 1);
    desk.command(Command::Lock(LockOrder::Sign { text }))
}

fn join(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    desk.command(Command::JoinCoinFile(first(args, "Name the file.")?.into()))
}

fn save_coin(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    desk.command(Command::SaveCoinFile(first(args, "Name the file.")?.into()))
}

fn id_text(id: &BoardId) -> String {
    id.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn boards(desk: &mut dyn Desk, _args: &Args) -> Result<String, String> {
    let rows = desk.boards()?;
    let lines = rows.iter().map(|view| {
        let state = if view.seen.online { "online" } else { "quiet" };
        format!("{}  {}  {state}", id_text(&view.seen.id), view.seen.beacon.name)
    });
    Ok(lines.collect::<Vec<_>>().join("\n"))
}

/// The board a word means: its name, or the start of its id.
pub(super) fn find_board(desk: &mut dyn Desk, word: Option<&str>) -> Result<BoardId, String> {
    let word = word.ok_or("Name a board: olc boards lists them.")?;
    let rows = desk.boards()?;
    let matches = |view: &&BoardView| {
        view.seen.beacon.name == word || id_text(&view.seen.id).starts_with(word)
    };
    let found = rows.iter().find(matches);
    found.map(|view| view.seen.id).ok_or_else(|| format!("No board called {word}."))
}

fn board(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let id = find_board(desk, args.word(1))?;
    let words: Vec<&str> = (2..).map_while(|index| args.word(index)).collect();
    let line = words.join(" ");
    let action = BoardAction::parse(&words).ok_or_else(|| format!("A board has no {line}."))?;
    if let Some(fault) = action.fault() {
        return Err(fault_text(fault).to_string());
    }
    desk.command(Command::Board { id, action })
}

fn update(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let id = find_board(desk, args.word(1))?;
    desk.command(Command::UpdateBoard(id))
}

fn move_board(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let id = find_board(desk, args.word(1))?;
    desk.command(Command::MoveBoard(id))
}

fn cap_test(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let id = find_board(desk, args.word(1))?;
    desk.command(Command::StartCapTest(id))
}

fn install(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let path = args.value("port").ok_or("--port needs a port")?.to_string();
    let name = args.value("name").ok_or("--name needs a name")?.to_string();
    desk.command(Command::InstallBoard { path, name })
}

fn lab_stop(desk: &mut dyn Desk, _args: &Args) -> Result<String, String> {
    desk.command(Command::StopExperiment)
}

fn node_start(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    desk.command(Command::StartNode(number(args.word(1))?))
}

fn node_stop(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    desk.command(Command::StopNode(number(args.word(1))?))
}

fn node_remove(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    desk.command(Command::RemoveNode(number(args.word(1))?))
}

fn node_cap(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let (id, cap) = (number(args.word(1))?, number(args.word(2))?);
    desk.command(Command::NodeCap { id, cap })
}

fn node_mine(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let on = match args.word(2) {
        Some("on") => true,
        Some("off") => false,
        _ => return Err("Say on or off.".into()),
    };
    desk.command(Command::NodeMining { id: number(args.word(1))?, on })
}

fn node_miner(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let to = args.word(2).ok_or("Say the address, or own.")?.to_string();
    desk.command(Command::NodeMiner { id: number(args.word(1))?, to })
}

fn node_pay(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let to = args.word(2).ok_or("Say the address.")?.to_string();
    let amount = args.word(3).ok_or("Say how much.")?.to_string();
    desk.command(Command::NodePay { id: number(args.word(1))?, to, amount })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Heard(Vec<Command>);

    impl Desk for Heard {
        fn command(&mut self, command: Command) -> Result<String, String> {
            self.0.push(command);
            Ok(String::new())
        }

        fn boards(&mut self) -> Result<Vec<BoardView>, String> {
            Ok(Vec::new())
        }

        fn password(&mut self) -> Result<String, String> {
            Ok("word".into())
        }
    }

    #[test]
    fn a_line_comes_to_the_command_a_button_sends() {
        let mut desk = Heard::default();
        run_line(&mut desk, "mine on").unwrap();
        run_line(&mut desk, "note two words").unwrap();
        run_line(&mut desk, "backup wallet.json --password").unwrap();
        assert!(matches!(desk.0[0], Command::SetMining(true)));
        assert!(matches!(&desk.0[1], Command::Own(Personal::WriteNote(text)) if text == "two words"));
        let backup = &desk.0[2];
        assert!(matches!(backup, Command::Own(Personal::BackUp { password: Some(_), .. })));
    }

    #[test]
    fn names_are_unique_and_a_stranger_is_refused() {
        let mut names: Vec<_> = WORDS.iter().map(|word| word.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), WORDS.len());
        assert!(run_line(&mut Heard::default(), "fly").is_err());
    }
}
