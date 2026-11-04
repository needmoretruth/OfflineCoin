//! `olc node`: one node in a terminal. It is the same host the app runs, with no window. It prints
//! a line when its chain moves, and takes the words of `olc` typed on its input.
use crate::coinfile;
use olc_board::Args;
use olc_link_serial::SerialLink;
use olc_node::{Card, Rank};
use olc_service::core::door::Door;
use olc_service::core::host_peers::Carried;
use olc_service::core::{
    device_kind, new_coin, parse_coins, passwords_asked, run_line, write_coin_file, BoardView, Book,
    Command, Desk, Host, Maker, NodeView, Notice, Personal, Plan,
};
use olc_store::NodeSettings;
use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

const NODE_PORT: u16 = 7000;
/// The cap of a computer whose owner never chose one.
const RELAXED_LIMIT: u64 = 20_000;
const LIMIT_FILE: &str = "hash-limit";
const LOOK_EVERY: Duration = Duration::from_millis(200);
/// Time for a stored chain to open.
const CHAIN_OPENS: Duration = Duration::from_secs(120);
/// Time for a lookup to reach this node's view.
const LOOKUP_SHOWS: Duration = Duration::from_millis(600);
/// Boards, the app's own nodes, pools and experiments belong to OLC Core.
const APP_ONLY: &str = "olc node has no boards or nodes of its own. Type that word as olc WORD in another terminal.";
const OWN_COIN: &str = "olc node runs the coin in its --coin file. Change the file, then start the node again.";
const STILL_OPENING: &str = "The chain is still opening. Try again in a moment.";
/// Swaps follow a second coin beside this one, which only OLC Core runs.
const SWAPS_IN_CORE: &str = "Swaps run in OLC Core, which follows the other coin beside this one. Open OLC Core on this wallet, then type this again: olc swap words reach it from any terminal.";

/// The order of links: this run's flag, saved if given. The host reads the saved choice.
fn save_links(args: &Args, settings: &mut NodeSettings) -> Result<(), String> {
    if let Some(text) = args.value("links") {
        let rank = Rank::parse(text).ok_or("--links needs wired, espnow, bluetooth and wifi, each once")?;
        settings.links = rank.text();
    }
    Ok(())
}

/// The cap to start with: this run's flag, saved if given, then the saved choice, then the relaxed
/// default.
fn chosen_limit(args: &Args, directory: &Path) -> Result<u64, String> {
    if args.value("hash-limit").is_some() {
        let limit = args.number("hash-limit", 0)?;
        save_limit(directory, limit);
        return Ok(limit);
    }
    let saved = fs::read_to_string(directory.join(LIMIT_FILE)).ok();
    Ok(saved.and_then(|text| text.trim().parse().ok()).unwrap_or(RELAXED_LIMIT))
}

/// A choice that cannot be saved still holds for this run.
fn save_limit(directory: &Path, limit: u64) {
    let _ = olc_store::replace_file(&directory.join(LIMIT_FILE), limit.to_string().as_bytes());
}

fn start_host(args: &Args, directory: PathBuf) -> Result<Host, String> {
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let coin = coinfile::load(args.value("coin"))?;
    let cap = chosen_limit(args, &directory)?;
    let mut settings = NodeSettings::load(&directory);
    settings.bluetooth |= args.has("ble");
    if let Some(card) = args.value("espnow") {
        settings.esp_now_card = card.to_string();
    }
    save_links(args, &mut settings)?;
    settings.save(&directory)?;
    let port = args.number("port", NODE_PORT)?;
    let peers = args.values("connect");
    let host = Host::start(
        Plan {
            coin,
            folder: directory.clone(),
            wallet_folder: directory,
            bind_host: args.value("bind").unwrap_or("0.0.0.0").to_string(),
            port,
            port_tries: 1,
            peers: peers.clone(),
            cap,
            mining: args.has("mine") || settings.mining,
            card: Card {
                device: device_kind(),
                name: args.value("name").unwrap_or_default().to_string(),
                build: env!("CARGO_PKG_VERSION").into(),
                ..Card::default()
            },
            storage: None,
            shares_wallet: false,
        },
        None,
    )?;
    if host.port.is_none() {
        return Err(format!("Port {port} is taken."));
    }
    host.set_known(peers);
    for path in args.values("serial") {
        let link = SerialLink::open(&path, |_| {}).map_err(|error| format!("{path}: {error}"))?;
        host.runner
            .lock()
            .map_err(|_| "The node stopped.".to_string())?
            .add_link(Box::new(link));
    }
    Ok(host)
}

/// Typed lines, in order. The passwords a word reads come right after it, and typed at a
/// terminal they do not show, which `wallet unlock` and `backup --password` need.
fn typed_lines() -> Receiver<String> {
    let (sender, lines) = channel();
    thread::spawn(move || {
        let hidden = std::io::stdin().is_terminal();
        let mut typed = std::io::stdin().lines().map_while(Result::ok);
        while let Some(line) = typed.next() {
            let asked = passwords_asked(&crate::joined_line(&line));
            if sender.send(line).is_err() {
                return;
            }
            for _ in 0..asked {
                let password = match hidden {
                    true => rpassword::read_password().ok(),
                    false => typed.next(),
                };
                if password.is_none_or(|password| sender.send(password).is_err()) {
                    return;
                }
            }
        }
    });
    lines
}

/// Typed words land on the node of this terminal.
struct NodeDesk<'a> {
    host: &'a Host,
    directory: &'a Path,
    lines: &'a Receiver<String>,
    /// The nodes `--connect` named, called again on `scan`.
    known: &'a [String],
    /// Blocks `generate` is making.
    maker: &'a mut Maker,
}

/// A notice as the answer to a typed word.
fn said(key: &'static str, detail: String, good: bool) -> Result<String, String> {
    let sentence = Notice { number: 0, key, detail, good }.sentence();
    if good {
        Ok(sentence)
    } else {
        Err(sentence)
    }
}

/// The answer the core gives to an order with no notice.
fn at_once(command: &Command) -> Result<String, String> {
    command.said_at_once().ok_or_else(|| "Done.".to_string())
}

impl NodeDesk<'_> {
    fn explore(&self, text: &str) -> Result<String, String> {
        self.host.show_explored(text)?;
        thread::sleep(LOOKUP_SHOWS);
        Ok(self.host.view().explored.text(text))
    }

    fn pay(&self, to: &str, amount: &str) -> Result<String, String> {
        let units = parse_coins(amount).filter(|units| *units > 0);
        let Some(units) = units else {
            return said("notice.bad_amount", String::new(), false);
        };
        match self.host.pay(to, units) {
            Ok(id) => {
                let id: String = id.iter().map(|byte| format!("{byte:02x}")).collect();
                said("notice.sent", self.host.amount_text(units), true).map(|sent| format!("{sent}\n{id}"))
            }
            Err(error) => {
                let (key, detail) = error.notice();
                said(key, detail, false)
            }
        }
    }
}

impl Desk for NodeDesk<'_> {
    fn command(&mut self, command: Command) -> Result<String, String> {
        // A word typed while the chain opens waits for it, as a one-shot word does for the core's.
        let started = Instant::now();
        let opening = |view: NodeView| view.coin.is_none() || view.loading;
        while opening(self.host.view()) && started.elapsed() < CHAIN_OPENS {
            thread::sleep(LOOK_EVERY);
        }
        let answer = at_once(&command);
        match command {
            Command::Own(Personal::Explore(text)) => self.explore(&text),
            Command::Own(order) => {
                let (key, detail, good) = self.host.obey(order);
                said(key, detail, good)
            }
            Command::Lock(order) => {
                let (key, detail, good) = self.host.carry_lock(order);
                said(key, detail, good)
            }
            Command::Channel(order) => {
                let (key, detail, good) = self.host.carry_channel(order);
                said(key, detail, good)
            }
            Command::SetMining(on) => {
                self.host.set_mining(on);
                let mut settings = NodeSettings::load(self.directory);
                settings.mining = on;
                settings.save(self.directory).and(answer)
            }
            Command::SetCap(cap) => {
                self.host.set_cap(cap);
                save_limit(self.directory, cap);
                answer
            }
            Command::Pay { to, amount } => self.pay(&to, &amount),
            Command::AddAddress(address) => {
                self.host.connect(address);
                answer
            }
            Command::Rescan => {
                self.known.iter().for_each(|address| self.host.connect(address.clone()));
                answer
            }
            Command::NewCoinFile { choices, path } => {
                let coin = new_coin(&choices).map_err(|bad| bad.to_string())?;
                match write_coin_file(&coin, &path) {
                    Ok(()) => said("notice.coin_saved", path, true),
                    Err(why) => said("notice.coin_file", why, false),
                }
            }
            Command::SaveCoinFile(path) => {
                let coin = self.host.view().coin.ok_or(STILL_OPENING)?;
                match write_coin_file(&coin, &path) {
                    Ok(()) => said("notice.coin_saved", path, true),
                    Err(why) => said("notice.coin_file", why, false),
                }
            }
            Command::Join { .. } | Command::JoinCoinFile(_) | Command::Fork { .. } | Command::ForkBack => {
                Err(OWN_COIN.to_string())
            }
            Command::Swap(_) => Err(SWAPS_IN_CORE.to_string()),
            Command::Chain(order) => {
                let (key, detail, good) = self.maker.order(self.host, order);
                said(key, detail, good)
            }
            _ => Err(APP_ONLY.to_string()),
        }
    }

    fn boards(&mut self) -> Result<Vec<BoardView>, String> {
        Err(APP_ONLY.to_string())
    }

    fn password(&mut self) -> Result<String, String> {
        println!("Password:");
        self.lines.recv().map_err(|_| "No password was given.".to_string())
    }
}

/// What happened to a channel since the last look, oldest first.
fn print_channel_news(host: &Host, heard: &mut u64) {
    let events = host.channels().events;
    for event in events.iter().rev().filter(|event| event.number > *heard) {
        let detail = event.detail.clone();
        println!("{}", Notice { number: event.number, key: event.key, detail, good: true }.sentence());
    }
    *heard = events.first().map_or(*heard, |event| event.number.max(*heard));
}

/// Traffic moving to another link is rare and worth a line in any log.
fn print_moves(host: &Host) {
    let Ok(mut runner) = host.runner.lock() else {
        return;
    };
    while let Some(moved) = runner.take_move() {
        // The network link is TCP whatever carries it, so it says so.
        let name = |link: Option<usize>| {
            link.and_then(|link| runner.medium_of(link)).map_or("nothing", |medium| Carried::of(medium).word())
        };
        println!("link {:x} {} -> {}", moved.peer, name(moved.from), name(moved.to));
    }
}

pub fn run(args: &Args) -> Result<(), String> {
    let directory = crate::wallet_folder(args)?;
    let known = args.values("connect");
    let host = start_host(args, directory.clone())?;
    let name = args.value("name").map(|name| format!("{name} ")).unwrap_or_default();
    println!("{name}listening on port {}", host.port.unwrap_or(0));
    let lines = typed_lines();
    // Words from `olc` in another terminal reach this node through its door.
    let door = Door::open(&directory);
    if door.is_none() {
        eprintln!("Another node runs on this wallet, so olc words from other terminals go to it.");
    }
    let mut shown = None;
    let mut maker = Maker::default();
    let mut typing_open = true;
    let mut heard = 0;
    loop {
        let typed = if typing_open {
            lines.recv_timeout(LOOK_EVERY)
        } else {
            thread::sleep(LOOK_EVERY);
            Err(RecvTimeoutError::Timeout)
        };
        match typed {
            Ok(line) if line.trim().is_empty() => {}
            Ok(line) => {
                let mut desk = NodeDesk { host: &host, directory: &directory, lines: &lines, known: &known, maker: &mut maker };
                let line = crate::joined_line(&line);
                match run_line(&mut desk, &line) {
                    Ok(answer) => println!("{answer}"),
                    Err(error) => println!("{error}"),
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            // A node with nobody at its input keeps running.
            Err(RecvTimeoutError::Disconnected) => typing_open = false,
        }
        if let Some(door) = &door {
            let mut desk = NodeDesk { host: &host, directory: &directory, lines: &lines, known: &known, maker: &mut maker };
            door.serve(&mut desk);
        }
        // Each block `generate` made is a line of this node's log, as its height is.
        for (key, detail, good) in maker.step(&host) {
            println!("{}", Notice { number: 0, key, detail, good }.sentence());
        }
        host.take_radio_notes().iter().for_each(|note| eprintln!("{note}"));
        print_moves(&host);
        print_channel_news(&host, &mut heard);
        let view = host.view();
        if view.coin.is_some() && shown != Some(view.height) {
            shown = Some(view.height);
            println!(
                "{name}height {} peers {} pool {} {} H/s",
                view.height, view.peers, view.pool, view.hashrate
            );
        }
    }
}
