//! Asking for help prints it and starts nothing, whatever command it follows, as `bitcoind -help` does.
use std::env::temp_dir;
use std::fs;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Far longer than printing takes, far shorter than a node that started would run.
const PRINTS_WITHIN: Duration = Duration::from_secs(20);

#[test]
fn help_after_any_command_prints_help_and_starts_nothing() {
    let home = temp_dir().join(format!("olc-help-{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(&home).unwrap();
    let asks: [&[&str]; 6] = [
        &["--help"],
        &["node", "--help"],
        &["node", "-h"],
        &["start", "--help"],
        &["devnet", "--help"],
        &["pay", "--help"],
    ];
    for words in asks {
        let mut olc = Command::new(env!("CARGO_BIN_EXE_olc"))
            .args(words)
            .arg("--root")
            .arg(home.join("data"))
            .env("HOME", &home)
            .env("OLC_NO_USB", "1")
            .current_dir(&home)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let started = Instant::now();
        let ended = loop {
            if olc.try_wait().unwrap().is_some() {
                break true;
            }
            if started.elapsed() > PRINTS_WITHIN {
                let _ = olc.kill();
                break false;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let out = olc.wait_with_output().unwrap();
        assert!(ended, "olc {} kept running instead of printing help", words.join(" "));
        let said = String::from_utf8_lossy(&out.stdout);
        assert!(said.contains("olc node"), "olc {} printed no help: {said}", words.join(" "));
        assert!(!home.join("data").exists(), "olc {} made a data folder", words.join(" "));
    }
    fs::remove_dir_all(&home).unwrap();
}
