//! Commands for a node's keys and choices. They work on its folder, so a running node reads them
//! when it next starts, and a secret key only ever goes to this terminal or a file named here.
use olc_board::Args;
use olc_store::{
    back_up, format_secret, load_wallet, new_secret, parse_secret, restore, save_wallet,
    NodeSettings,
};
use olc_wallet::{format_address, parse_address, Unsigned, Wallet};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;

fn folder(args: &Args) -> PathBuf {
    PathBuf::from(args.value("data").unwrap_or(".olc/node"))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn list(wallet: &Wallet) {
    for (index, (address, public)) in wallet.keys().enumerate() {
        let mark = if index == wallet.receiving() { "*" } else { " " };
        println!("{mark} {index}  {}  {}", format_address(&address), hex(&public));
    }
}

fn password(args: &Args) -> Result<Option<String>, String> {
    if !args.has("password") {
        return Ok(None);
    }
    print!("Password: ");
    std::io::stdout().flush().map_err(|error| error.to_string())?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).map_err(|error| error.to_string())?;
    Ok(Some(line.trim_end().to_string()))
}

fn key(args: &Args, wallet: &mut Wallet) -> Result<bool, String> {
    let number = |word: Option<&str>| word.and_then(|text| text.parse::<usize>().ok());
    match args.word(1) {
        Some("new") => {
            let index = wallet.add_secret(new_secret()?).map_err(|e| format!("{e:?}"))?;
            println!("{index}");
            Ok(true)
        }
        Some("use") => {
            let index = number(args.word(2)).ok_or("Name a key: olc key use 1")?;
            wallet.set_receiving(index).map_err(|_| "There is no such key.")?;
            Ok(true)
        }
        Some("import") => {
            let secret = args.word(2).and_then(parse_secret).ok_or("That is not a secret key.")?;
            let index = wallet.add_secret(secret).map_err(|e| format!("{e:?}"))?;
            println!("{index}");
            Ok(true)
        }
        Some("export") => {
            let index = number(args.word(2)).ok_or("Name a key: olc key export 1")?;
            let secret = wallet.export_secret(index).ok_or("There is no such key.")?;
            println!("{}", format_secret(&secret));
            Ok(false)
        }
        _ => Err("Use: olc key new | use N | import HEX | export N".into()),
    }
}

/// Merges keys from a backup into the wallet, so restoring never loses a key.
fn merge_keys(wallet: &mut Wallet, restored: &Wallet) -> usize {
    let secrets = (0..restored.key_count()).filter_map(|index| restored.export_secret(index));
    secrets.filter(|secret| wallet.add_secret(*secret).is_ok()).count()
}

fn send_note(args: &Args) -> Result<(), String> {
    let text = args.word(1).ok_or("Write the note: olc note \"hello\"")?;
    let body = serde_json::json!({ "text": text }).to_string();
    println!("{}", post(args, "note", &body)?);
    Ok(())
}

/// Asks the node on this computer.
fn post(args: &Args, path: &str, body: &str) -> Result<String, String> {
    let port: u16 = args.number("web", 8000)?;
    let mut stream =
        TcpStream::connect(("127.0.0.1", port)).map_err(|_| "No node runs on this computer.")?;
    let request = format!(
        "POST /api/{path} HTTP/1.0\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).map_err(|error| error.to_string())?;
    let mut reply = String::new();
    stream.read_to_string(&mut reply).map_err(|error| error.to_string())?;
    Ok(reply.split("\r\n\r\n").nth(1).unwrap_or(&reply).to_string())
}

/// `olc prepare TO UNITS`: a payment from the watched addresses, for an offline wallet to sign.
fn prepare(args: &Args) -> Result<(), String> {
    let to = args.word(1).ok_or("Name the address: olc prepare ADDRESS UNITS")?;
    let amount = args.word(2).and_then(|word| word.parse::<u64>().ok());
    let amount = amount.ok_or("Name the amount in units: olc prepare ADDRESS UNITS")?;
    let fee: u64 = args.number("fee", 0)?;
    let body = serde_json::json!({ "to": to, "amount": amount, "fee": fee }).to_string();
    println!("{}", post(args, "prepare", &body)?);
    Ok(())
}

/// `olc sign FILE`: prints the signed payment. Touches no network.
fn sign(args: &Args, wallet: &Wallet) -> Result<(), String> {
    let path = args.word(1).ok_or("Name the file: olc sign unsigned.json")?;
    let text = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let reply: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let unsigned: Unsigned =
        serde_json::from_value(reply["unsigned"].clone()).map_err(|e| e.to_string())?;
    let signed = wallet.sign(&unsigned).map_err(|error| format!("{error:?}"))?;
    println!("{}", serde_json::to_string(&signed).map_err(|error| error.to_string())?);
    Ok(())
}

fn broadcast(args: &Args) -> Result<(), String> {
    let path = args.word(1).ok_or("Name the file: olc broadcast signed.json")?;
    let body = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    println!("{}", post(args, "broadcast", body.trim())?);
    Ok(())
}

fn choice(args: &Args, word: &str) -> Result<(), String> {
    let directory = folder(args);
    let mut settings = NodeSettings::load(&directory);
    let value = args.word(1).ok_or("Say what to set.")?;
    match word {
        "miner" if value == "off" => settings.miner = None,
        "miner" => {
            let address = parse_address(value).ok_or("That is not an address.")?;
            settings.miner = Some(format_address(&address));
        }
        "keep" if value == "all" => settings.recent = None,
        "keep" => {
            let blocks = value.parse().map_err(|_| "Say how many blocks, or all.")?;
            settings.recent = Some(blocks);
        }
        "merge" => settings.no_merge = value == "off",
        "light" => settings.light = value == "on",
        "mining" => settings.mining = value == "on",
        "watch" => {
            parse_address(&value).ok_or("That is not an address.")?;
            settings.watched.push(value.to_string());
        }
        _ => settings.note = value.to_string(),
    }
    settings.save(&directory)
}

pub fn run(word: &str, args: &Args) -> Result<(), String> {
    let directory = folder(args);
    match word {
        "note" => return send_note(args),
        "prepare" => return prepare(args),
        "broadcast" => return broadcast(args),
        "miner" | "keep" | "merge" | "mine-note" | "light" | "mining" | "watch" => {
            return choice(args, word);
        }
        _ => {}
    }
    let mut wallet = load_wallet(&directory)?;
    match word {
        "keys" => list(&wallet),
        "sign" => sign(args, &wallet)?,
        "key" => {
            if key(args, &mut wallet)? {
                save_wallet(&directory, &wallet)?;
                list(&wallet);
            }
        }
        "backup" => {
            let path = args.word(1).ok_or("Name the file: olc backup wallet.json")?;
            back_up(&wallet, path.as_ref(), password(args)?.as_deref())?;
            println!("{path}");
        }
        _ => {
            let path = args.word(1).ok_or("Name the file: olc restore wallet.json")?;
            let restored = restore(path.as_ref(), password(args)?.as_deref())?;
            println!("{}", merge_keys(&mut wallet, &restored));
            save_wallet(&directory, &wallet)?;
            list(&wallet);
        }
    }
    Ok(())
}
