//! `olc node`: one node in a terminal. It is the same host the app runs, with no window. It prints
//! a line when its chain moves, and takes the words of `olc` typed on its input.
use crate::coinfile;
use olc_board::Args;
use olc_link_serial::SerialLink;
use olc_node::{Card, Medium, Rank};
use olc_service::core::{
    device_kind, run_line, BoardView, Book, Command, Desk, Host, Personal, Plan,
};
use olc_store::NodeSettings;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::thread;
use std::time::Duration;

const NODE_PORT: u16 = 7000;
/// The cap of a computer whose owner never chose one.
const RELAXED_LIMIT: u64 = 20_000;
const LIMIT_FILE: &str = "hash-limit";
const LOOK_EVERY: Duration = Duration::from_millis(200);
/// Time for a lookup to reach this node's view.
const LOOKUP_SHOWS: Duration = Duration::from_millis(600);
const NOT_HERE: &str = "That needs OLC Core or `olc` with no node running.";

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
    let _ = fs::write(directory.join(LIMIT_FILE), limit.to_string());
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

fn typed_lines() -> Receiver<String> {
    let (sender, lines) = channel();
    thread::spawn(move || {
        for line in std::io::stdin().lines().map_while(Result::ok) {
            if sender.send(line).is_err() {
                return;
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
}

impl NodeDesk<'_> {
    fn explore(&self, text: &str) -> Result<String, String> {
        self.host.show_explored(text)?;
        thread::sleep(LOOKUP_SHOWS);
        Ok(format!("{:#?}", self.host.view().explored))
    }
}

impl Desk for NodeDesk<'_> {
    fn command(&mut self, command: Command) -> Result<String, String> {
        match command {
            Command::Own(Personal::Explore(text)) => self.explore(&text),
            Command::Own(order) => {
                let (key, detail, good) = self.host.obey(order);
                let said = key.trim_start_matches("notice.").replace('_', " ");
                let answer = if detail.is_empty() { said } else { detail };
                if good {
                    Ok(answer)
                } else {
                    Err(answer)
                }
            }
            Command::SetMining(on) => {
                self.host.set_mining(on);
                let mut settings = NodeSettings::load(self.directory);
                settings.mining = on;
                settings.save(self.directory).map(|()| "saved".to_string())
            }
            Command::SetCap(cap) => {
                self.host.set_cap(cap);
                save_limit(self.directory, cap);
                Ok("saved".to_string())
            }
            Command::Pay { to, amount } => {
                let units = olc_service::core::parse_coins(&amount).filter(|units| *units > 0);
                let units = units.ok_or("That is not an amount.")?;
                let id = self.host.pay(&to, units).map_err(|error| format!("{error:?}"))?;
                Ok(id.iter().map(|byte| format!("{byte:02x}")).collect())
            }
            Command::AddAddress(address) => {
                self.host.connect(address);
                Ok("saved".to_string())
            }
            _ => Err(NOT_HERE.to_string()),
        }
    }

    fn boards(&mut self) -> Result<Vec<BoardView>, String> {
        Err(NOT_HERE.to_string())
    }

    fn password(&mut self) -> Result<String, String> {
        println!("Password:");
        self.lines.recv().map_err(|_| "No password was given.".to_string())
    }
}

/// Traffic moving to another link is rare and worth a line in any log.
fn print_moves(host: &Host) {
    let Ok(mut runner) = host.runner.lock() else {
        return;
    };
    while let Some(moved) = runner.take_move() {
        let name = |link: Option<usize>| {
            link.and_then(|link| runner.medium_of(link)).map_or("nothing", Medium::name)
        };
        println!("link {:x} {} -> {}", moved.peer, name(moved.from), name(moved.to));
    }
}

pub fn run(args: &Args) -> Result<(), String> {
    let directory = PathBuf::from(args.value("data").unwrap_or(".olc/node"));
    let host = start_host(args, directory.clone())?;
    let name = args.value("name").map(|name| format!("{name} ")).unwrap_or_default();
    println!("{name}listening on port {}", host.port.unwrap_or(0));
    let lines = typed_lines();
    let mut shown = None;
    let mut typing_open = true;
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
                let mut desk = NodeDesk { host: &host, directory: &directory, lines: &lines };
                match run_line(&mut desk, &line) {
                    Ok(answer) => println!("{answer}"),
                    Err(error) => println!("{error}"),
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            // A node with nobody at its input keeps running.
            Err(RecvTimeoutError::Disconnected) => typing_open = false,
        }
        host.take_radio_notes().iter().for_each(|note| eprintln!("{note}"));
        print_moves(&host);
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
