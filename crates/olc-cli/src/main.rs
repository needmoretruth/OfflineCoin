//! `olc`: OfflineCoin for a terminal. Every action is the core's. This file only reads the flags
//! that say where to work and prints what comes back.
mod app;
mod coinfile;
mod devnet;
mod fleet_api;
mod logo;
mod node;
mod web;
mod window;

use olc_board::{Args, Extra};
use olc_service::core::{run_words, usage, Folder, Terminal};
use olc_service::local::run_node;
use std::io::Write;
use std::path::PathBuf;

const PROGRAMS: &str = "\
OfflineCoin

  olc start   --data DIR --wallet DIR --coin FILE --port 7000 [--bind HOST] [--connect HOST:PORT]...
              [--cap HASHES] [--mine]      a node through the core
  olc node    [--coin FILE] [--data DIR] [--port 7000] [--connect HOST:PORT]...
              [--serial <port>]... [--web 8000] [--web-public]
              [--mine] [--hash-limit HASHES_PER_SECOND]
  olc app     [--firmware FILE] [--serial PORT]... [--web 8000] [--data DIR]
  olc devnet  [--nodes 3] [--coin FILE] [--hash-limit HASHES_PER_SECOND]
  olc logo    [--once]

Each of these also takes --data DIR, --root DIR and --serial PORT:

";

const CLOSING: &str = "\nA coin file is JSON. Edit it to change any rule.\n";

/// Optional programs. Nothing else in this program names one.
pub const EXTRAS: &[&dyn Extra] = &[&olc_lab::Lab];

fn asked_password() -> Result<String, String> {
    print!("Password: ");
    std::io::stdout().flush().map_err(|error| error.to_string())?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).map_err(|error| error.to_string())?;
    Ok(line.trim_end().to_string())
}

fn terminal(args: &Args) -> Result<Terminal, String> {
    let folder = Folder {
        directory: PathBuf::from(args.value("data").unwrap_or(".olc/node")),
        web_port: args.number("web", 8000)?,
    };
    let mut terminal = Terminal::new(folder);
    terminal.root = args.value("root").map(PathBuf::from);
    terminal.ports = args.values("serial");
    terminal.firmware = args.value("firmware").map(str::to_string);
    if args.has("password") {
        terminal.given_password = Some(asked_password()?);
    }
    Ok(terminal)
}

fn action(args: &Args) -> Result<(), String> {
    match run_words(&mut terminal(args)?, args) {
        Some(answer) => {
            println!("{}", answer?);
            Ok(())
        }
        None => match EXTRAS.iter().find(|extra| Some(extra.command()) == args.command()) {
            Some(extra) => extra.run(args),
            None => {
                print!("{PROGRAMS}{}", usage());
                EXTRAS.iter().for_each(|extra| print!("{}", extra.usage()));
                print!("{CLOSING}");
                Ok(())
            }
        },
    }
}

fn main() {
    let args = Args::parse(std::env::args().skip(1));
    let result = match args.command() {
        Some("node") => node::run(&args),
        Some("app") | None => node::run_app(&args),
        Some("start") => run_node(std::env::args().skip(2).collect()),
        Some("devnet") => devnet::run(&args),
        Some("logo") => logo::run(&args),
        _ => action(&args),
    };
    if let Err(message) = result {
        eprintln!("{message}");
        std::process::exit(1);
    }
}
