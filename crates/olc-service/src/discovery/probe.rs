//! Asks one address what it is, over an outward connection.
use olc_node::frame::{encode, Decoder};
use olc_node::network::{read, stamp, OPEN};
use olc_node::{Hello, Message};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

const REPLY_WAIT: Duration = Duration::from_millis(1_500);
const READ_SIZE: usize = 16 * 1024;
/// A node sends chain traffic too, and nothing else this large is worth reading for.
const MAX_BYTES: usize = 256 * 1024;

fn first_address(address: &str) -> Option<SocketAddr> {
    address.to_socket_addrs().ok()?.next()
}

/// `None` when nothing answers or what answers is no OLC node.
pub fn ask(address: &str, connect_wait: Duration) -> Option<Hello> {
    let target = first_address(address)?;
    let mut stream = TcpStream::connect_timeout(&target, connect_wait).ok()?;
    let question = stamp(OPEN, &Message::Who.encode());
    stream.write_all(&encode(&question)).ok()?;
    stream.set_read_timeout(Some(REPLY_WAIT)).ok()?;
    let deadline = Instant::now() + REPLY_WAIT;
    let mut decoder = Decoder::default();
    let mut bytes = vec![0u8; READ_SIZE];
    let mut total = 0;
    while Instant::now() < deadline && total < MAX_BYTES {
        let size = stream.read(&mut bytes).ok().filter(|size| *size > 0)?;
        total += size;
        decoder.push(&bytes[..size]);
        while let Some(message) = decoder.next_message() {
            let Some((mark, body)) = read(&message) else {
                continue;
            };
            if mark != OPEN {
                continue;
            }
            if let Some(Message::Hello(hello)) = Message::decode(body) {
                return Some(*hello);
            }
        }
        if decoder.is_broken() {
            return None;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Host, Plan};
    use olc_core::CoinConfig;
    use olc_node::Card;

    /// A real node on a real socket says what it is to someone it has never met, and its coin
    /// needs no file because the answer holds it.
    #[test]
    fn a_node_answers_who_is_there() {
        let folder = std::env::temp_dir().join(format!("olc-probe-{}", std::process::id()));
        let mut coin = CoinConfig::olc();
        coin.name = "Asked".into();
        let host = Host::start(
            Plan {
                coin: coin.clone(),
                folder: folder.join("chain"),
                wallet_folder: folder.join("wallet"),
                bind_host: "127.0.0.1".into(),
                port: 7340,
                port_tries: 20,
                peers: Vec::new(),
                cap: 0,
                mining: false,
                card: Card {
                    name: "shelf".into(),
                    device: "computer".into(),
                    ..Card::default()
                },
                storage: None,
                shares_wallet: false,
            },
            None,
        )
        .unwrap();
        let mut port = None;
        for _ in 0..100 {
            port = host.view().port.filter(|_| host.view().coin.is_some());
            if port.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let address = format!("127.0.0.1:{}", port.unwrap());
        let hello = ask(&address, Duration::from_secs(2)).expect("an answer");
        assert_eq!(hello.card.name, "shelf");
        assert_eq!(hello.coin, coin);
        assert_eq!(hello.genesis(), coin.genesis().id());
        drop(host);
        let _ = std::fs::remove_dir_all(folder);
    }
}
