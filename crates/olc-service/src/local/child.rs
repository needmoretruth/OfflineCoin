//! The node process: the same host the app runs, with no window. It takes one command per line on
//! its input, prints its status as one JSON line a second, and ends when its input closes, so no
//! node outlives the app that started it.
use crate::core::Book;
use super::supervisor::ChildStatus;
use crate::core::{parse_coins, Host, Plan};
use crate::settings::PORT_TRIES;
use olc_core::BlockHeader;
use olc_node::Card;
use olc_store::coin_file;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::thread;
use std::time::Duration;

const STATUS_EVERY: Duration = Duration::from_secs(1);

#[derive(Default)]
struct Words {
    data: PathBuf,
    wallet: PathBuf,
    coin: PathBuf,
    bind: String,
    port: u16,
    connect: Vec<String>,
    cap: u64,
    storage: Option<u64>,
    name: String,
    mining: bool,
}

fn parse(words: Vec<String>) -> Result<Words, String> {
    let mut parsed = Words {
        bind: "127.0.0.1".into(),
        port: crate::settings::NODE_PORT,
        ..Words::default()
    };
    let mut words = words.into_iter();
    while let Some(word) = words.next() {
        if word == "--mine" {
            parsed.mining = true;
            continue;
        }
        let value = words.next().ok_or_else(|| format!("{word} needs a value"))?;
        let number = |text: &str| text.parse::<u64>().map_err(|_| format!("{word}: {text}"));
        match word.as_str() {
            "--data" => parsed.data = value.into(),
            "--wallet" => parsed.wallet = value.into(),
            "--coin" => parsed.coin = value.into(),
            "--bind" => parsed.bind = value,
            "--port" => parsed.port = number(&value)? as u16,
            "--connect" => parsed.connect.push(value),
            "--cap" => parsed.cap = number(&value)?,
            "--storage" => parsed.storage = Some(number(&value)?),
            "--name" => parsed.name = value,
            _ => return Err(format!("unknown word {word}")),
        }
    }
    Ok(parsed)
}

fn read_lines() -> Receiver<String> {
    let (sender, lines) = channel();
    thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else {
                break;
            };
            if sender.send(line).is_err() {
                return;
            }
        }
        // Closed input ends the node. A closed channel says so.
    });
    lines
}

fn say(status: &ChildStatus) {
    if let Ok(line) = serde_json::to_string(status) {
        let mut out = std::io::stdout().lock();
        // A reader that left means the app is gone too.
        if writeln!(out, "{line}").and_then(|()| out.flush()).is_err() {
            std::process::exit(0);
        }
    }
}

fn run_line(host: &Host, line: &str) -> Option<ChildStatus> {
    let words: Vec<&str> = line.split_whitespace().collect();
    match words.as_slice() {
        ["limit", cap] => host.set_cap(cap.parse().ok()?),
        ["mine", on] => host.set_mining(*on == "on"),
        ["mineto", "own"] => drop(host.set_miner("")),
        ["mineto", address] => drop(host.set_miner(address)),
        ["work", "stop"] => host.take_work(None),
        ["work", header, difficulty] => {
            let header = unhex(header).and_then(|bytes| BlockHeader::decode(&bytes));
            host.take_work(Some((header?, difficulty.parse().ok()?)));
        }
        ["pay", to, amount] => {
            let paid = parse_coins(amount)
                .ok_or_else(|| "bad_amount".to_string())
                .and_then(|units| host.pay(to, units).map_err(|error| format!("{error:?}")));
            let mut status = ChildStatus::from_view(&host.view());
            match paid {
                Ok(id) => status.paid = Some(hex(&id)),
                Err(error) => status.error = Some(error),
            }
            return Some(status);
        }
        _ => {}
    }
    None
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    let bytes = (0..text.len() / 2).map(|at| u8::from_str_radix(text.get(at * 2..at * 2 + 2)?, 16).ok());
    bytes.collect()
}

fn status_of(host: &Host) -> ChildStatus {
    let mut status = ChildStatus::from_view(&host.view());
    status.shares = host.take_shares().iter().map(|header| hex(&header.encode())).collect();
    status
}

/// `olc-core node ...`: runs until told to stop or until its input closes.
pub fn run_node(words: Vec<String>) -> Result<(), String> {
    let words = parse(words)?;
    let text = std::fs::read_to_string(&words.coin).map_err(|error| error.to_string())?;
    let coin = coin_file::from_json(&text)?;
    let host = Host::start(
        Plan {
            coin,
            folder: words.data,
            wallet_folder: words.wallet,
            bind_host: words.bind,
            port: words.port,
            port_tries: PORT_TRIES,
            peers: words.connect,
            cap: words.cap,
            mining: words.mining,
            card: Card {
                name: words.name,
                device: crate::core::device_kind(),
                build: env!("CARGO_PKG_VERSION").into(),
                ..Card::default()
            },
            storage: words.storage,
        },
        None,
    )?;
    let lines = read_lines();
    loop {
        match lines.recv_timeout(STATUS_EVERY) {
            Ok(line) if line == "quit" => return Ok(()),
            Ok(line) => {
                if let Some(status) = run_line(&host, &line) {
                    say(&status);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
        }
        say(&status_of(&host));
    }
}
