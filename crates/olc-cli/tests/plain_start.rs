//! Typing the program's name only explains it. Boards are taken when someone asks for that.
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(20);

#[test]
fn a_plain_start_shows_the_words_and_ends() {
    let mut program = Command::new(env!("CARGO_BIN_EXE_olc"))
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let started = Instant::now();
    while program.try_wait().unwrap().is_none() {
        if started.elapsed() > WAIT {
            program.kill().unwrap();
            panic!("a plain start kept running");
        }
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn the_words_name_one_terminal_program_and_no_page() {
    let shown = Command::new(env!("CARGO_BIN_EXE_olc")).arg("help").output().unwrap();
    let words = String::from_utf8_lossy(&shown.stdout);
    assert!(words.contains("olc node"), "{words}");
    assert!(words.contains("olc devnet"), "{words}");
    for gone in ["olc app", "--web", "http"] {
        assert!(!words.contains(gone), "{gone} is back: {words}");
    }
}

#[test]
fn the_old_app_word_only_shows_the_words() {
    let shown = Command::new(env!("CARGO_BIN_EXE_olc")).arg("app").output().unwrap();
    assert!(shown.status.success());
    assert!(String::from_utf8_lossy(&shown.stdout).contains("olc node"));
}
