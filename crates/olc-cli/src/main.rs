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
  olc --version

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

/// The two-word commands, spelled as one: `olc channel open …` is `olc channel-open …`, as
/// Lightning's programs spell it apart, and `olc pool join …` is `olc pool-join …`.
fn one_word(first: &str, second: &str) -> Option<String> {
    match (first, second) {
        ("channel", "open" | "close" | "pay") | ("pool", "start" | "stop" | "join" | "leave") => {
            Some(format!("{first}-{second}"))
        }
        ("channel", "list") => Some("channels".into()),
        _ => None,
    }
}

pub fn joined(words: Vec<String>) -> Vec<String> {
    let pair = words.first().map(String::as_str).zip(words.get(1).map(String::as_str));
    match pair.and_then(|(first, second)| one_word(first, second)) {
        Some(word) => std::iter::once(word).chain(words.into_iter().skip(2)).collect(),
        None => words,
    }
}

/// The same for a typed line, which keeps its quotes.
pub fn joined_line(line: &str) -> String {
    let Some((first, rest)) = line.trim_start().split_once(' ') else {
        return line.to_string();
    };
    let rest = rest.trim_start();
    let (second, after) = rest.split_once(' ').unwrap_or((rest, ""));
    match one_word(first, second) {
        Some(word) if after.is_empty() => word,
        Some(word) => format!("{word} {after}"),
        None => line.to_string(),
    }
}

fn terminal(args: &Args) -> Result<Terminal, String> {
    let directory = wallet_folder(args)?;
    let mut terminal = Terminal::new(Folder { directory });
    terminal.words = joined(std::env::args().skip(1).collect());
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
    let args = Args::parse(joined(std::env::args().skip(1).collect()).into_iter());
    // Asking for help never starts or changes anything, whatever command it follows.
    if args.has("help") || std::env::args().skip(1).any(|word| word == "-h") {
        help();
        return;
    }
    if args.has("version") || std::env::args().skip(1).any(|word| word == "-V") {
        println!("olc {}", env!("CARGO_PKG_VERSION"));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_word_commands_reach_their_words() {
        let words = |line: &str| joined(line.split(' ').map(str::to_string).collect()).join(" ");
        assert_eq!(words("pool join own 192.168.0.5 addr.rig"), "pool-join own 192.168.0.5 addr.rig");
        assert_eq!(words("pool start --open"), "pool-start --open");
        assert_eq!(words("channel open key@host:1 5"), "channel-open key@host:1 5");
        assert_eq!(words("channel list"), "channels");
        assert_eq!(words("pool"), "pool");
        assert_eq!(joined_line("pool stop"), "pool-stop");
        assert_eq!(joined_line("channel list --all"), "channels --all");
        assert_eq!(joined_line("  invoice 5 \"for tea\""), "  invoice 5 \"for tea\"");
        assert_eq!(joined_line("channel pay \"lnolc1 x\""), "channel-pay \"lnolc1 x\"");
    }
}
