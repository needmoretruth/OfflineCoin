//! Proves on a real board that a locked coin stays put when someone offers the wrong unlock
//! words. The app never sends such a spend, because it checks first; a stranger on the radio
//! would, so this sends it straight down the cable and prints what the board says back.
//! Run with no app holding the port:
//!   wrongunlock PORT TXID:INDEX VALUE_UNITS "LOCK WORDS" "WRONG UNLOCK WORDS"
use olc_core::{OutPoint, TxOut};
use olc_lang::words::lock_from_text;
use olc_link_serial::SerialLink;
use olc_node::{network, Link, Message, Target};
use olc_wallet::locks::LockSpend;
use std::thread;
use std::time::{Duration, Instant};

const HELLO_WAIT: Duration = Duration::from_secs(30);
const ANSWER_WAIT: Duration = Duration::from_secs(12);
const LOOK: Duration = Duration::from_millis(2);

fn hash_of(text: &str) -> Option<olc_core::Hash> {
    let bytes: Option<Vec<u8>> = (0..text.len() / 2)
        .map(|at| u8::from_str_radix(text.get(at * 2..at * 2 + 2)?, 16).ok())
        .collect();
    bytes?.try_into().ok()
}

fn spend(coin: &str, value: &str, lock: &str, unlock: &str) -> Result<olc_core::Tx, String> {
    let (txid, index) = coin.rsplit_once(':').ok_or("COIN is TXID:INDEX")?;
    let txid = hash_of(txid).ok_or("TXID is 64 hex digits")?;
    let previous = OutPoint { txid, index: index.parse().map_err(|_| "bad index")? };
    let (lock, _) = lock_from_text(lock).map_err(|error| format!("{error:?}"))?;
    let value: u64 = value.parse().map_err(|_| "bad value")?;
    let coin = TxOut { value, language: olc_lang::BETA, lock };
    let fee = olc_core::coin::COIN / 100;
    // The words are wrong on purpose and carry no signature, so no coin is named.
    let spend = LockSpend::new(previous, &coin, [0; 20], fee, unlock, None).map_err(|error| format!("{error:?}"))?;
    spend.finish().map_err(|error| format!("{error:?}"))
}

/// The coin's mark, from the board's own hello.
fn mark_of_board(link: &mut SerialLink) -> Result<network::Mark, String> {
    let start = Instant::now();
    link.send(Target::All, &network::stamp(network::OPEN, &Message::Who.encode()));
    while start.elapsed() < HELLO_WAIT {
        while let Some((_, bytes)) = link.receive() {
            let message = network::read(&bytes).and_then(|(_, body)| Message::decode(body));
            if let Some(Message::Hello(hello)) = message {
                println!("board {} {} height {}", hello.card.name, hello.card.build, hello.height);
                return Ok(network::mark_of(&hello.genesis()));
            }
        }
        thread::sleep(LOOK);
    }
    Err("the board did not say hello".into())
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [port, coin, value, lock, unlock] = args.as_slice() else {
        return Err("wrongunlock PORT TXID:INDEX VALUE_UNITS \"LOCK\" \"WRONG UNLOCK\"".into());
    };
    let tx = spend(coin, value, lock, unlock)?;
    let mut link = SerialLink::open(port, |line| {
        if !line.starts_with("status") {
            println!("  board says: {line}");
        }
    })
    .map_err(|error| error.to_string())?;
    let mark = mark_of_board(&mut link)?;
    link.send(Target::All, &network::stamp(mark, &Message::Tx(tx).encode()));
    println!("sent a spend of {coin} with the unlock words {unlock:?}");
    let start = Instant::now();
    while start.elapsed() < ANSWER_WAIT {
        while link.receive().is_some() {}
        thread::sleep(LOOK);
    }
    Ok(())
}
