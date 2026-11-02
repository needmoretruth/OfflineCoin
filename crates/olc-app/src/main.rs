#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
//! `olc-core`: the OLC Core desktop app, and the same program as a node process of its own.
use olc_app::app::{run, Launch};
use olc_app::local::run_node;

fn main() {
    let mut words = std::env::args().skip(1).peekable();
    if words.peek().is_some_and(|word| word == "node") {
        let result = run_node(words.skip(1).collect());
        finish(result);
        return;
    }
    let mut launch = Launch::default();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--no-usb" => launch.usb = false,
            "--data" => launch.data = words.next().map(Into::into),
            "--port" => launch.ports.extend(words.next()),
            _ => {
                println!("OLC Core\n\n  olc-core [--no-usb] [--port FILE]... [--data DIR]");
                return;
            }
        }
    }
    if let Err(message) = run(launch) {
        eprintln!("{message}");
        // Opened from a menu or a desktop icon, the app has no terminal to print to.
        let shown = format!("OLC Core could not start.\n\n{message}");
        olc_service::platform().show_error("OLC Core", &shown);
        std::process::exit(1);
    }
}

fn finish(result: Result<(), String>) {
    if let Err(message) = result {
        eprintln!("{message}");
        std::process::exit(1);
    }
}
