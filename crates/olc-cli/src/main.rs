//! `olc`: OfflineCoin for a terminal. Every action is the core's. This file only reads the flags
//! that say where to work and prints what comes back.
mod coinfile;
mod devnet;
mod logo;
mod node;

use olc_board::{Args, Extra};
use olc_service::core::{run_words, usage, Folder, Terminal};
use olc_service::local::run_node;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;

const PROGRAMS: &str = "\
OfflineCoin

  olc start   --data DIR --wallet DIR --coin FILE --port 7000 [--bind HOST] [--connect HOST:PORT]...
              [--cap HASHES] [--mine]      a node through the core
  olc node    [--coin FILE] [--data DIR] [--root DIR] [--port 7000] [--bind HOST] [--connect HOST:PORT]...
              [--serial <port>]... [--name NAME] [--links ORDER] [--ble] [--espnow CARD]
              [--mine] [--hash-limit HASHES_PER_SECOND]
              a node in this terminal: type the words below to act on it
  olc devnet  [--nodes 3] [--coin FILE] [--hash-limit HASHES_PER_SECOND]
  olc logo    [--once]

Without --data, olc and olc node use the wallet of OLC Core.
Each of these also takes --data DIR, --root DIR and --serial PORT:

";

const CLOSING: &str = "\nA coin file is JSON. Edit it to change any rule.\n";

/// Optional programs. Nothing else in this program names one.
pub const EXTRAS: &[&dyn Extra] = &[&olc_lab::Lab];

/// Typed at a terminal, a password does not show. A script can still pipe one in.
fn asked_password() -> Result<String, String> {
    print!("Password: ");
    std::io::stdout().flush().map_err(|error| error.to_string())?;
    if std::io::stdin().is_terminal() {
        return rpassword::read_password().map_err(|error| error.to_string());
    }
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).map_err(|error| error.to_string())?;
    Ok(line.trim_end().to_string())
}

/// The folder of the one wallet this run reads, with a word for anyone whose wallet sits where
/// `olc` kept it before.
pub fn wallet_folder(args: &Args) -> Result<PathBuf, String> {
    let root = args.value("root").map(PathBuf::from);
    let directory = Terminal::wallet_folder(args.value("data"), root.as_deref())?;
    if args.value("data").is_none() {
        Terminal::old_wallet_here(&directory).iter().for_each(|line| eprintln!("{line}"));
    }
    Ok(directory)
}

fn terminal(args: &Args) -> Result<Terminal, String> {
    let directory = wallet_folder(args)?;
    let mut terminal = Terminal::new(Folder { directory });
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
                help();
                Ok(())
            }
        },
    }
}

fn help() {
    print!("{PROGRAMS}{}", usage());
    EXTRAS.iter().for_each(|extra| print!("{}", extra.usage()));
    print!("{CLOSING}");
}

fn main() {
    let args = Args::parse(std::env::args().skip(1));
    // Asking for help never starts or changes anything, whatever command it follows.
    if args.has("help") || std::env::args().skip(1).any(|word| word == "-h") {
        help();
        return;
    }
    let result = match args.command() {
        Some("node") => node::run(&args),
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
