//! Every field of a coin goes from the real `olc new-coin` through a file into a running node. The
//! list comes from the table a test in `olc-store` holds to the coin's own fields.
use olc_core::CoinConfig;
use olc_service::core::{Command as Order, Core, CoreLaunch, COIN_FIELDS, WORDS};
use olc_service::settings::{Paths, Settings};
use olc_wallet::format_address;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(60);

/// A value for each field that is not what a new coin has by itself.
fn sample(key: &str) -> String {
    match key {
        "name" => "우리 코인".into(),
        "ticker" => "WLK".into(),
        "message" => "우리 코인 첫 블록".into(),
        "message_in_block" => "off".into(),
        "genesis_time_ms" => "1234567".into(),
        "initial_difficulty" => "321".into(),
        "premine" => format!("{} 12.5", format_address(&[9; 20])),
        "rules.block_time_ms" => "200".into(),
        "rules.retarget_interval" => "7".into(),
        "rules.reward" => "0.25".into(),
        "rules.halving_interval" => "11".into(),
        "rules.coinbase_maturity" => "13".into(),
        "rules.max_block_bytes" => "9000".into(),
        "rules.min_fee" => "0.001".into(),
        "rules.min_difficulty" => "17".into(),
        "rules.floor_from_height" => "19".into(),
        other => panic!("{other} has no sample, so nothing proves it reaches a coin"),
    }
}

fn fresh(name: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn every_field_goes_from_the_word_to_a_running_coin() {
    let folder = fresh("every_coin_field");
    let file = folder.join("walk.json");
    let mut word = Command::new(env!("CARGO_BIN_EXE_olc"));
    word.env("HOME", &folder).env("XDG_DATA_HOME", &folder).env("XDG_CONFIG_HOME", &folder);
    word.arg("new-coin").arg(&file);
    for field in COIN_FIELDS {
        word.arg(format!("--{}", field.flag)).arg(sample(field.key));
    }
    let run = word.output().unwrap();
    let said = String::from_utf8_lossy(&run.stderr);
    assert!(run.status.success(), "{said}");

    let paths = Paths { root: folder.join("app") };
    std::fs::create_dir_all(&paths.root).unwrap();
    let settings = Settings { setup_done: true, scan_lan: false, broadcast: false, port: 0, ..Settings::default() };
    let launch = CoreLaunch { paths, fleet: None, usb: false, node_program: None };
    let core = Core::start(launch, settings).unwrap();
    core.send(Order::JoinCoinFile(file.display().to_string()));

    let is_ours = |coin: &CoinConfig| coin.ticker == "WLK";
    let started = Instant::now();
    let running = loop {
        let coin = core.snapshot().node.coin.clone();
        if let Some(coin) = coin.filter(is_ours) {
            break coin;
        }
        assert!(started.elapsed() < WAIT, "the node never took the coin");
        thread::sleep(Duration::from_millis(50));
    };
    for field in COIN_FIELDS {
        assert_eq!((field.get)(&running), sample(field.key), "{}", field.key);
    }
}

#[test]
fn the_word_tells_of_every_flag() {
    let word = WORDS.iter().find(|word| word.name == "new-coin").unwrap();
    for field in COIN_FIELDS {
        assert!(word.usage.contains(&format!("--{}", field.flag)), "{} is untold", field.flag);
    }
}

#[test]
fn a_bad_value_gets_a_plain_refusal() {
    let folder = fresh("bad_coin_field");
    let file = folder.join("bad.json");
    let mut word = Command::new(env!("CARGO_BIN_EXE_olc"));
    word.env("HOME", &folder).env("XDG_DATA_HOME", &folder).env("XDG_CONFIG_HOME", &folder);
    let run = word.arg("new-coin").arg(&file).args(["--name", "x", "--block-time-ms", "0"]).output().unwrap();
    assert!(!run.status.success());
    assert!(!file.exists());
    assert!(!String::from_utf8_lossy(&run.stderr).trim().is_empty());
}
