//! `olc`: run a node, start a test network or write a new coin file.
mod app;
mod coinfile;
mod devnet;
mod fleet_api;
mod logo;
mod node;
mod web;
mod window;

use olc_board::{Args, Extra};

const USAGE: &str = "\
OfflineCoin

  olc node    [--coin FILE] [--data DIR] [--port 7000] [--connect HOST:PORT]...
              [--serial /dev/ttyACM0]... [--web 8000] [--web-public]
              [--mine] [--hash-limit HASHES_PER_SECOND]
  olc app     [--firmware FILE] [--serial PORT]... [--web 8000] [--data DIR]
  olc install --port PORT --name NAME [--firmware FILE]
  olc devnet  [--nodes 3] [--coin FILE] [--hash-limit HASHES_PER_SECOND]
  olc new-coin FILE [--name NAME] [--ticker TICKER] [--block-time-ms MS]
              [--reward COINS] [--halving BLOCKS] [--difficulty N]
  olc logo    [--once]
";

const CLOSING: &str = "\nA coin file is JSON. Edit it to change any rule.\n";

/// Optional programs. Nothing else in this program names one.
pub const EXTRAS: &[&dyn Extra] = &[&olc_lab::Lab];

fn main() {
    let args = Args::parse(std::env::args().skip(1));
    let result = match args.command() {
        Some("node") => node::run(&args),
        Some("app") | None => node::run_app(&args),
        Some("install") => app::install(&args),
        Some("devnet") => devnet::run(&args),
        Some("new-coin") => coinfile::create(&args),
        Some("logo") => logo::run(&args),
        word => match EXTRAS.iter().find(|extra| Some(extra.command()) == word) {
            Some(extra) => extra.run(&args),
            None => {
                print!("{USAGE}");
                EXTRAS.iter().for_each(|extra| print!("{}", extra.usage()));
                print!("{CLOSING}");
                Ok(())
            }
        },
    };
    if let Err(message) = result {
        eprintln!("{message}");
        std::process::exit(1);
    }
}
