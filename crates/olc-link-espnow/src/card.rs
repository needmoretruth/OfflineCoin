//! The card. It must already be in monitor mode on the boards' channel, which takes root once,
//! and this program needs only `CAP_NET_RAW`.
use crate::air::{frame, read, BROADCAST, MOST_BODY};
use olc_node::fragment::{split, Reassembler};
use olc_node::network::{self, Mark};
use olc_node::{Link, Medium, Target};
use pnet_datalink::{channel, interfaces, Channel, Config, DataLinkSender};
use std::collections::BTreeMap;
use std::sync::mpsc::{channel as queue, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

type Macs = Arc<Mutex<BTreeMap<u32, [u8; 6]>>>;

pub struct EspNowCard {
    to_card: Box<dyn DataLinkSender>,
    from_card: Receiver<(u32, Vec<u8>)>,
    me: [u8; 6],
    macs: Macs,
    next_message: u16,
}

/// The same number a board gives a sender.
fn number_of(mac: &[u8; 6]) -> u32 {
    u32::from_be_bytes([0, mac[3], mac[4], mac[5]])
}

/// How long a card may keep a frame waiting. The node stands still for that long.
const WRITE_WAIT: Duration = Duration::from_millis(20);
/// How soon to read again from a card that failed.
const CARD_BACK: Duration = Duration::from_secs(1);

impl EspNowCard {
    /// Opens `interface`, and hears frames of this coin and everyone's Hellos.
    pub fn open(interface: &str, mark: Mark) -> Result<Self, String> {
        let card = interfaces().into_iter().find(|card| card.name == interface);
        let card = card.ok_or_else(|| format!("no card named {interface}"))?;
        let me = card.mac.map(|mac| mac.octets()).ok_or("the card has no address")?;
        let config = Config { read_timeout: None, write_timeout: Some(WRITE_WAIT), ..Config::default() };
        let (to_card, mut card_reader) = match channel(&card, config) {
            Ok(Channel::Ethernet(sending, reading)) => (sending, reading),
            Ok(_) => return Err("the card offers no raw frames".into()),
            Err(error) => return Err(format!("{interface}: {error}")),
        };
        let (heard, from_card) = queue();
        let macs = Macs::default();
        let known = macs.clone();
        thread::spawn(move || {
            let mut joiner = Reassembler::default();
            loop {
                // A card that went down and came back up reports it once, and carries on.
                let Ok(bytes) = card_reader.next() else {
                    thread::sleep(CARD_BACK);
                    continue;
                };
                let Some((from, body)) = read(bytes, &me) else {
                    continue;
                };
                match network::read(body) {
                    Some((found, _)) if found == mark => {}
                    Some((network::OPEN, _)) => {}
                    _ => continue,
                }
                let number = number_of(&from);
                if let Ok(mut known) = known.lock() {
                    known.insert(number, from);
                }
                if let Some(message) = joiner.push(number, body) {
                    if heard.send((number, message)).is_err() {
                        return;
                    }
                }
            }
        });
        Ok(Self { to_card, from_card, me, macs, next_message: 0 })
    }
}

impl Link for EspNowCard {
    fn medium(&self) -> Medium {
        Medium::EspNow
    }

    fn send(&mut self, target: Target, message: &[u8]) {
        let chosen = match target {
            Target::Peer(number) => self.macs.lock().ok().and_then(|macs| macs.get(&number).copied()),
            Target::All | Target::AllExcept(_) => None,
        };
        let to = chosen.unwrap_or(BROADCAST);
        self.next_message = self.next_message.wrapping_add(1);
        for piece in split(self.next_message, message, MOST_BODY) {
            let bytes = frame(&to, &self.me, &piece);
            // A card that stopped taking frames must not stop the node. The rest of a message
            // would be of no use without this piece.
            if !matches!(self.to_card.send_to(&bytes, None), Some(Ok(()))) {
                return;
            }
        }
    }

    fn receive(&mut self) -> Option<(u32, Vec<u8>)> {
        self.from_card.try_recv().ok()
    }
}
