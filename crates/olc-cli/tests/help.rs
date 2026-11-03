//! Asking for help prints it and starts nothing, whatever command it follows, as `bitcoind -help` does.
use std::env::temp_dir;
use std::fs;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// A last guard only: a node that started is caught at once by the data folder it makes. Printing
/// takes milliseconds, but twenty and then a hundred and twenty seconds both ran out while the
/// machine's build jobs sat at their memory limit.
const PRINTS_WITHIN: Duration = Duration::from_secs(600);

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
        // Read while it runs, so a run that hangs still tells whether it printed first.
        let mut stdout = olc.stdout.take().unwrap();
        let reader = std::thread::spawn(move || {
            let mut text = String::new();
            let _ = std::io::Read::read_to_string(&mut stdout, &mut text);
            text
        });
        let started = Instant::now();
        let ended = loop {
            if olc.try_wait().unwrap().is_some() {
                break true;
            }
            if started.elapsed() > PRINTS_WITHIN || home.join("data").exists() {
                let _ = olc.kill();
                break false;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let _ = olc.wait();
        let said = reader.join().unwrap();
        let took = started.elapsed();
        assert!(ended, "olc {} kept running for {took:?}; it printed {} bytes first", words.join(" "), said.len());
        assert!(said.contains("olc node"), "olc {} printed no help: {said}", words.join(" "));
        assert!(!home.join("data").exists(), "olc {} made a data folder", words.join(" "));
    }
    fs::remove_dir_all(&home).unwrap();
}
