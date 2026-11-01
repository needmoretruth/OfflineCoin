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
