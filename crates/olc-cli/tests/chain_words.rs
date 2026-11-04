//! The words a Bitcoin Core user types first, against a running node from another terminal:
//! `olc generate N` mines N blocks for real at full speed and says how far it is, `olc coins`
//! lists the wallet's coins with amount, confirmations and address as listunspent does, and
//! `olc mempool` lists the payments waiting for a block as getrawmempool does. The node stays on
//! 127.0.0.1 and has a home of its own.
use std::env::temp_dir;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{id, Child, Command, Output, Stdio};
use std::sync::mpsc::channel;
use std::thread;
use std::time::{Duration, Instant};

const STARTS: Duration = Duration::from_secs(60);
const MAKES: Duration = Duration::from_secs(120);

fn home(name: &str) -> PathBuf {
    let home = temp_dir().join(format!("olc-chain-words-{}-{name}", id()));
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(home.join("data")).unwrap();
    fs::create_dir_all(home.join("work")).unwrap();
    home
}

fn olc(home: &Path, words: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_olc"));
    command.args(words).arg("--root").arg(home.join("data"));
    command.env("HOME", home).env("OLC_NO_USB", "1").current_dir(home.join("work"));
    command
}

fn run(home: &Path, words: &[&str]) -> Output {
    olc(home, words).output().unwrap()
}

fn said(output: &Output) -> String {
    let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).trim().to_string();
    format!("{}{}", text(&output.stdout), text(&output.stderr))
}

/// A node process that ends with the test, passed or not.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// `olc node` on `port` on a coin made here, once it listens.
fn node(home: &Path, port: u16, coin: &Path) -> Running {
    let port = port.to_string();
    let coin = coin.to_string_lossy().to_string();
    let words = ["node", "--bind", "127.0.0.1", "--port", port.as_str(), "--coin", coin.as_str()];
    let mut node = olc(home, &words).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    let out = node.stdout.take().unwrap();
    let (said, heard) = channel();
    thread::spawn(move || {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            let _ = said.send(line);
        }
    });
    let started = Instant::now();
    loop {
        assert!(started.elapsed() < STARTS, "the node never listened");
        if heard.recv_timeout(Duration::from_secs(1)).is_ok_and(|line| line.contains("listening on port")) {
            break;
        }
    }
    thread::spawn(move || while heard.recv().is_ok() {});
    Running(node)
}

/// Asks until the answer holds `part`.
fn until(home: &Path, words: &[&str], part: &str, within: Duration) -> String {
    let started = Instant::now();
    loop {
        let answer = said(&run(home, words));
        if answer.contains(part) {
            return answer;
        }
        assert!(started.elapsed() < within, "olc {words:?} never said {part:?}: {answer}");
        thread::sleep(Duration::from_millis(500));
    }
}

#[test]
fn blocks_are_made_now_and_coins_and_waiting_payments_are_listed_from_another_terminal() {
    let home = home("a");
    let coin = home.join("work").join("easy.json");
    let coin_text = coin.to_string_lossy().to_string();
    // A coin mined in a moment, whose mined coins move after one block.
    let made = run(&home, &["new-coin", &coin_text, "--name", "Words", "--ticker", "WRD", "--difficulty", "2000", "--min-difficulty", "1", "--maturity", "1"]);
    assert!(made.status.success(), "{}", said(&made));
    let running = node(&home, 7791, &coin);

    let none = run(&home, &["generate", "stop"]);
    assert!(!none.status.success(), "{}", said(&none));
    assert!(said(&none).contains("No blocks are being made"), "{}", said(&none));

    let started = run(&home, &["generate", "3"]);
    assert!(started.status.success(), "{}", said(&started));
    assert!(said(&started).contains("Making 3 blocks at full speed"), "{}", said(&started));
    let done = until(&home, &["generate"], "3 of 3 blocks made.", MAKES);
    assert_eq!(done.lines().filter(|line| line.trim_start().starts_with("block ")).count(), 3, "{done}");

    // Exactly three, even on a coin this easy, and mining rests as it did before.
    thread::sleep(Duration::from_secs(3));
    let coins = said(&run(&home, &["coins"]));
    assert!(coins.starts_with("3 coins, 150 in all."), "{coins}");
    let lines: Vec<&str> = coins.lines().collect();
    assert!(lines[1].contains("1 confirmation ") || lines[1].contains("1 confirmation\t"), "{coins}");
    assert!(lines[5].contains("3 confirmations"), "{coins}");
    let address = lines[5].split_whitespace().nth(3).expect("the address a coin pays").to_string();
    assert!(address.len() > 20, "{coins}");

    assert_eq!(said(&run(&home, &["mempool"])), "No payments are waiting for a block.");
    let paid = run(&home, &["pay", &address, "1"]);
    assert!(paid.status.success(), "{}", said(&paid));
    let waiting = until(&home, &["mempool"], "1 payment waiting for a block", STARTS);
    assert_eq!(waiting.lines().count(), 2, "{waiting}");
    assert!(waiting.lines().nth(1).unwrap().contains("bytes  fee "), "{waiting}");

    // A long making ends by hand, and the node says what it made so far.
    let long = run(&home, &["generate", "1000"]);
    assert!(long.status.success(), "{}", said(&long));
    let stopped = run(&home, &["generate", "stop"]);
    assert!(said(&stopped).contains("Stopped making blocks."), "{}", said(&stopped));
    assert!(!run(&home, &["generate", "0"]).status.success());

    drop(running);
    fs::remove_dir_all(&home).unwrap();
}
