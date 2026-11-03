//! `olc` words typed in another terminal reach the node already running on the same wallet, as
//! bitcoin-cli reaches bitcoind, and `olc channel open KEY@HOST:PORT` reaches a node it has never
//! met. Each node has a home of its own and stays on 127.0.0.1.
use std::env::temp_dir;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{id, Child, Command, Output, Stdio};
use std::sync::mpsc::channel;
use std::thread;
use std::time::{Duration, Instant};

const STARTS: Duration = Duration::from_secs(60);
/// Hello every five seconds, and a few rounds of it.
const MEETS: Duration = Duration::from_secs(40);

fn home(name: &str, port: u16) -> PathBuf {
    let home = temp_dir().join(format!("olc-running-{}-{name}", id()));
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(home.join("data")).unwrap();
    fs::create_dir_all(home.join("work")).unwrap();
    let settings = format!(r#"{{"listen_lan":false,"scan_lan":false,"port":{port}}}"#);
    fs::write(home.join("data").join("core.json"), settings).unwrap();
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

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim().to_string()
}

/// `olc node` on `port`, once it listens. Its typing stays open so it keeps running.
fn node(home: &Path, port: u16) -> Child {
    let port = port.to_string();
    let words = ["node", "--bind", "127.0.0.1", "--port", port.as_str()];
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
    // What it prints later is not read, so the pipe is kept drained.
    thread::spawn(move || while heard.recv().is_ok() {});
    node
}

/// The key of the node running on `home`, asked of that node.
fn key_of(home: &Path) -> String {
    let started = Instant::now();
    loop {
        let listed = run(home, &["channel", "list"]);
        let said = text(&listed.stdout);
        let key = said.lines().find_map(|line| line.strip_prefix("This node: ")).map(str::to_string);
        if let Some(key) = key.filter(|key| key.len() == 66) {
            return key;
        }
        assert!(started.elapsed() < STARTS, "no key from the running node: {said} {}", text(&listed.stderr));
        thread::sleep(Duration::from_millis(500));
    }
}

#[test]
fn a_channel_opens_by_address_from_the_terminal_against_running_nodes() {
    let (a, b) = (home("a", 7781), home("b", 7782));
    let (mut node_a, mut node_b) = (node(&a, 7781), node(&b, 7782));
    let key = key_of(&a);

    let address = format!("{key}@127.0.0.1:7781");
    let opened = run(&b, &["channel", "open", &address, "5"]);
    let said = text(&opened.stdout);
    assert!(opened.status.success(), "{said} {}", text(&opened.stderr));
    assert!(said.contains("Connecting to 127.0.0.1:7781"), "{said}");

    // The two had never met; now each hears the other's channel node.
    let started = Instant::now();
    let nearby = format!("Nearby: {}", &key[..16]);
    loop {
        let listed = text(&run(&b, &["channels"]).stdout);
        if listed.contains(&nearby) {
            break;
        }
        assert!(started.elapsed() < MEETS, "b never heard a: {listed}");
        thread::sleep(Duration::from_millis(500));
    }

    // Swap words reach the running node too, which says where swaps run.
    let swap = run(&b, &["swap", "list"]);
    assert!(!swap.status.success());
    assert!(text(&swap.stderr).contains("Swaps run in OLC Core"), "{}", text(&swap.stderr));

    for node in [&mut node_a, &mut node_b] {
        node.kill().unwrap();
        node.wait().unwrap();
    }
    // With the node gone, the same word is carried out here again.
    let alone = run(&b, &["swap", "list"]);
    assert!(alone.status.success(), "{}", text(&alone.stderr));
    fs::remove_dir_all(&a).unwrap();
    fs::remove_dir_all(&b).unwrap();
}
