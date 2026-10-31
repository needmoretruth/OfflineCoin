//! `olc`: run a node, start a test network or write a new coin file.
mod args;
mod coinfile;
mod devnet;
mod logo;
mod node;
mod store;
mod web;

use args::Args;

const USAGE: &str = "\
OfflineCoin

  olc node    [--coin FILE] [--data DIR] [--port 7000] [--connect HOST:PORT]...
              [--serial /dev/ttyACM0]... [--web 8000] [--web-public]
              [--mine] [--hash-limit HASHES_PER_SECOND]
  olc devnet  [--nodes 3] [--coin FILE] [--hash-limit HASHES_PER_SECOND]
  olc new-coin FILE [--name NAME] [--ticker TICKER] [--block-time-ms MS]
              [--reward COINS] [--halving BLOCKS] [--difficulty N]
  olc logo    [--once]

A coin file is JSON. Edit it to change any rule.
";

fn main() {
    let args = Args::parse(std::env::args().skip(1));
    let result = match args.command() {
        Some("node") => node::run(&args),
        Some("devnet") => devnet::run(&args),
        Some("new-coin") => coinfile::create(&args),
        Some("logo") => logo::run(&args),
        _ => {
            print!("{USAGE}");
            Ok(())
        }
    };
    if let Err(message) = result {
        eprintln!("{message}");
        std::process::exit(1);
    }
}
