//! One wallet for every word, the same words in a running `olc node`, and answers a person reads.
//! Each run has a home of its own, works away from its data folder, and stays on 127.0.0.1.
use olc_service::core::WORDS;
use olc_store::load_wallet;
use olc_wallet::format_address;
use std::env::temp_dir;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{id, Command, Output, Stdio};
use std::sync::mpsc::channel;
use std::thread;
use std::time::{Duration, Instant};

/// A line no other word prints, typed between words so each answer stands alone.
const BETWEEN: &str = "lock-check HASH 0x00 EQUAL";
const BETWEEN_SAYS: &str = "4 bytes, 3 words, 0 signature checks";
const NODE_ANSWERS: Duration = Duration::from_secs(90);

/// A home, a data folder in it that keeps to this computer, and a folder to run from.
fn home(name: &str, port: u16) -> PathBuf {
    let home = temp_dir().join(format!("olc-one-wallet-{}-{name}", id()));
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

/// A name from the code, such as `NotEnoughCoins` or `BlockDetail`, or a structure printed whole.
fn reads_as_code(answer: &str) -> bool {
    // The first block carries the coin's own name, which is not code.
    let answer = answer.replace(&olc_core::CoinConfig::olc().name, "");
    let camel = answer.split(|c: char| !c.is_ascii_alphabetic()).any(|word| {
        let mut letters = word.chars();
        letters.next().is_some_and(|first| first.is_ascii_uppercase())
            && word.chars().any(|c| c.is_ascii_lowercase())
            && word.chars().skip(1).any(|c| c.is_ascii_uppercase())
    });
    camel || answer.contains('{') || answer.contains('[') || answer.contains("notice.")
}

#[test]
fn keys_and_a_lock_spend_from_one_shot_read_one_wallet() {
    let home = home("one-shot", 7751);
    let made = run(&home, &["key", "new"]);
    assert!(made.status.success(), "{}", text(&made.stderr));
    let address = text(&made.stdout);
    let listed = text(&run(&home, &["keys"]).stdout);
    assert!(listed.contains(&address), "{listed}");
    // With no node near, the core opens the wallet and sends nothing.
    let signed = run(&home, &["lock-sign", "nothing"]);
    assert!(!signed.status.success());
    // The wallet the core opened for the spend holds the key the folder made.
    let wallet = load_wallet(&home.join("data").join("node")).unwrap();
    let held: Vec<String> = wallet.keys().map(|(key, _)| format_address(&key)).collect();
    assert!(held.contains(&address), "{held:?} lacks {address}");
    assert!(!home.join("work").join(".olc").exists(), "a second wallet appeared");
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn a_word_for_another_wallet_than_the_cores_starts_nothing() {
    let home = home("other", 7752);
    let other = home.join("other");
    let refused = run(&home, &["lock-sign", "nothing", "--data", other.to_str().unwrap()]);
    assert!(!refused.status.success());
    assert!(text(&refused.stderr).contains("olc node --data"), "{}", text(&refused.stderr));
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn a_block_and_an_address_read_as_lines_from_one_shot() {
    let home = home("explore", 7753);
    let block = run(&home, &["explore", "0"]);
    let lines = text(&block.stdout);
    assert!(lines.starts_with("block       0\nid          "), "{lines}");
    assert!(!reads_as_code(&lines), "{lines}");
    let address = text(&run(&home, &["key", "new"]).stdout);
    let found = text(&run(&home, &["explore", &address]).stdout);
    assert!(found.contains(&format!("address     {address}")), "{found}");
    assert!(!reads_as_code(&found), "{found}");
    fs::remove_dir_all(&home).unwrap();
}

/// A line that runs `word` in a node. Every word has one, so a new word is tried here too.
fn sample(word: &str, home: &Path) -> String {
    let file = |name: &str| home.join(name).display().to_string();
    let somebody = "olc409c2c2374917964bd7d010ddb77f90500eb3a41";
    match word {
        "keys" | "scan" | "fork-back" | "boards" | "pool" | "pool-stop" | "lab-stop" => word.to_string(),
        "key" => "key new".into(),
        "backup" => format!("backup {}", file("backup.json")),
        "restore" => format!("restore {}", file("backup.json")),
        "miner" => "miner off".into(),
        "keep" => "keep 100".into(),
        "merge" | "version1" | "bluetooth" | "light" | "mining" | "mine" => format!("{word} off"),
        "espnow" => "espnow off".into(),
        "watch" => format!("watch {somebody}"),
        "prepare" => format!("prepare {somebody} 5"),
        "sign" | "broadcast" => format!("{word} {}", file("none.json")),
        "note" | "mine-note" => format!("{word} hello"),
        "snapshot" => "snapshot accept".into(),
        "explore" => "explore 0".into(),
        "cap" => "cap 100".into(),
        "pay" => format!("pay {somebody} 5"),
        "add" => "add 127.0.0.1:7999".into(),
        "new-coin" => format!("new-coin {}", file("new.json")),
        "fork" => "fork --reward 2".into(),
        "lock-preset" => "lock-preset hash secret".into(),
        "lock-check" => "lock-check HASH 0x0102 EQUAL".into(),
        "lock-pay" => "lock-pay 1 HASH 0x00 EQUAL".into(),
        "lock-spend" => format!("lock-spend {}:0 x", "aa".repeat(32)),
        "lock-sign" => "lock-sign nothing".into(),
        "swap" => "swap list".into(),
        "join" => format!("join {}", file("new.json")),
        "save-coin" => format!("save-coin {}", file("saved.json")),
        "board" => "board x name y".into(),
        "update" | "move" | "cap-test" => format!("{word} x"),
        "install" => "install --port none --name x".into(),
        // Port 0: the system picks a free one, and the pool listens on 127.0.0.1 alone.
        "pool-start" => "pool-start --port 0".into(),
        "pool-join" => format!("pool-join own 127.0.0.1:9 {somebody}.rig"),
        "pool-leave" => "pool-leave own".into(),
        "node-start" | "node-stop" | "node-remove" => format!("{word} 1"),
        "node-cap" => "node-cap 1 1".into(),
        "node-mine" => "node-mine 1 on".into(),
        "node-miner" => "node-miner 1 own".into(),
        "node-pay" => format!("node-pay 1 {somebody} 1"),
        "channels" => word.to_string(),
        "channel-open" => "channel-open x 5".into(),
        "channel-close" => "channel-close x".into(),
        "invoice" => "invoice 5 hello".into(),
        "channel-pay" => "channel-pay x".into(),
        "peers" => word.to_string(),
        "disconnect" => "disconnect 99".into(),
        "ban" => "ban 192.0.2.7 1d".into(),
        "unban" => "unban 192.0.2.7".into(),
        "words" | "shared" => word.to_string(),
        // How far blocks being made are, without making any.
        "mempool" | "coins" | "generate" => word.to_string(),
        // A signature is base64, which reads like code to this test, so the line asks for a key
        // this wallet does not hold.
        "sign-message" => format!("sign-message --address {somebody} hello"),
        "verify-message" => format!("verify-message {somebody} AAAA hello"),
        "speed-up" => format!("speed-up {}", "aa".repeat(32)),
        other => panic!("no line tries {other}"),
    }
}

/// Words about the app's boards and its own nodes, and words that change the coin a node runs.
fn not_for_a_lone_node(word: &str) -> bool {
    let app = ["boards", "board", "update", "move", "cap-test", "install", "pool", "pool-start", "pool-stop", "pool-join", "pool-leave", "swap"];
    let own = ["lab-stop", "join", "fork", "fork-back"];
    app.contains(&word) || own.contains(&word) || word.starts_with("node-")
}

/// Types every line into `olc node` and gives what it answered to each.
fn typed_into_node(home: &Path, lines: &[String]) -> Vec<String> {
    let mut node = olc(home, &["node", "--bind", "127.0.0.1", "--port", "7754"]);
    let mut node = node.stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    let (said, heard) = channel();
    let out = node.stdout.take().unwrap();
    thread::spawn(move || {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            let _ = said.send(line);
        }
    });
    let mut typing = node.stdin.take().unwrap();
    for line in lines {
        writeln!(typing, "{line}\n{BETWEEN}").unwrap();
    }
    let (mut answers, mut current) = (Vec::new(), Vec::new());
    let started = Instant::now();
    while answers.len() < lines.len() && started.elapsed() < NODE_ANSWERS {
        let Ok(line) = heard.recv_timeout(Duration::from_secs(1)) else { continue };
        if line.contains("listening on port") || line.starts_with("height ") {
            continue;
        }
        if line == BETWEEN_SAYS {
            answers.push(current.join("\n"));
            current.clear();
        } else {
            current.push(line);
        }
    }
    node.kill().unwrap();
    node.wait().unwrap();
    answers
}

#[test]
fn every_word_typed_into_a_node_does_what_it_does_on_its_own_and_reads_plainly() {
    let home = home("node", 7755);
    let names: Vec<&str> = WORDS.iter().map(|word| word.name).collect();
    let lines: Vec<String> = names.iter().map(|name| sample(name, &home)).collect();
    let answers = typed_into_node(&home, &lines);
    assert_eq!(answers.len(), lines.len(), "{answers:?}");
    let mut wrong = Vec::new();
    for ((name, line), answer) in names.iter().zip(&lines).zip(&answers) {
        let refused = answer.contains("has no boards") || answer.contains("--coin file");
        let missing = answer.contains("needs OLC Core") || (refused && !not_for_a_lone_node(name));
        // The folder this test made is in some answers, and its name on Windows has AppData in it.
        let said = answer.replace(&home.display().to_string(), "home");
        if answer.is_empty() || missing || reads_as_code(&said) {
            wrong.push(format!("{line} -> {answer}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    // The node and a word typed later on its own read the same keys.
    let in_node = &answers[names.iter().position(|name| *name == "keys").unwrap()];
    let alone = text(&run(&home, &["keys"]).stdout);
    assert!(!in_node.is_empty() && alone.contains(in_node.as_str()), "{in_node}\n{alone}");
    fs::remove_dir_all(&home).unwrap();
}
