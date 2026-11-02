//! What the look around has found so far, remembered between looks.
use super::rows::BOARD_KIND;
use crate::discovery::{reach_of, Entry, Identity, Reach, Work};
use olc_core::Address;
use olc_node::hello::MAX_KNOWN;
use olc_node::{Hello, Medium};
use olc_node::Keeps;
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

/// Silent this long, a node passed on by others leaves the list. Added addresses stay.
const FORGET_AFTER: Duration = Duration::from_secs(600);
/// Boards speak over their links with no address to file them under.
const SILENT_AFTER: Duration = Duration::from_secs(120);
/// A connected node answers every two seconds. Older than this, a row says how old it is.
const LIVE_WITHIN: Duration = Duration::from_secs(5);
/// Rows for nodes heard or passed on. Anyone can claim any address, so the one heard longest ago
/// gives way. What a person added stays whatever the count.
const MOST_KNOWN: usize = 512;
/// Boards and computers heard over links. Anyone in radio reach can make these up too.
const MOST_ON_LINKS: usize = 128;

struct Known {
    hello: Option<Hello>,
    via: Option<String>,
    heard: Instant,
    added: bool,
}

#[derive(Default)]
pub struct Found {
    known: BTreeMap<String, Known>,
    /// What boards said over their links, by the address they mine to.
    boards: BTreeMap<Address, Hello>,
    /// Computers a board hears, which have no address on this network either.
    computers: BTreeMap<Address, (Hello, Instant)>,
}

fn room_for<T>(list: &mut BTreeMap<Address, T>, address: &Address) {
    if list.len() >= MOST_ON_LINKS && !list.contains_key(address) {
        list.pop_first();
    }
}

/// The link the board that passed this on hears it over.
pub fn link_of(hello: &Hello) -> Option<Medium> {
    Medium::from_name(&hello.card.via.0)
}

fn device_ip(address: &str) -> Option<std::net::IpAddr> {
    address
        .parse::<SocketAddr>()
        .ok()
        .map(|address| address.ip())
}

impl Found {
    fn room_for_one_more(&mut self, address: &str) {
        if self.known.len() < MOST_KNOWN || self.known.contains_key(address) {
            return;
        }
        let strangers = self.known.iter().filter(|(_, known)| !known.added);
        let oldest = strangers.min_by_key(|(_, known)| known.heard);
        if let Some(oldest) = oldest.map(|(address, _)| address.clone()) {
            self.known.remove(&oldest);
        }
    }

    pub fn added(&mut self, address: &str) {
        self.known.entry(address.to_string()).or_insert(Known {
            hello: None,
            via: None,
            heard: Instant::now(),
            added: true,
        });
        if let Some(known) = self.known.get_mut(address) {
            known.added = true;
        }
    }

    /// Takes in an answer and returns the addresses it passed on.
    pub fn heard(&mut self, address: &str, hello: Hello) -> Vec<(String, String)> {
        let name = hello.card.name.clone();
        let passed: Vec<(String, String)> = hello
            .card
            .known
            .iter()
            .take(MAX_KNOWN)
            .map(|known| (known.clone(), name.clone()))
            .collect();
        self.room_for_one_more(address);
        let entry = self.known.entry(address.to_string()).or_insert(Known {
            hello: None,
            via: None,
            heard: Instant::now(),
            added: false,
        });
        entry.hello = Some(hello);
        entry.heard = Instant::now();
        passed
    }

    pub fn board_said(&mut self, hello: Hello) {
        if hello.card.device == BOARD_KIND {
            room_for(&mut self.boards, &hello.address);
            self.boards.insert(hello.address, hello);
        } else {
            room_for(&mut self.computers, &hello.address);
            self.computers.insert(hello.address, (hello, Instant::now()));
        }
    }

    pub fn board(&self, address: &Address) -> Option<&Hello> {
        self.boards.get(address)
    }

    /// Notes who passed an address on, the first time.
    pub fn passed_on(&mut self, address: &str, via: &str) {
        self.room_for_one_more(address);
        self.known.entry(address.to_string()).or_insert(Known {
            hello: None,
            via: Some(via.to_string()),
            heard: Instant::now(),
            added: false,
        });
    }

    pub fn forget_old(&mut self) {
        self.known
            .retain(|_, known| known.added || known.heard.elapsed() < FORGET_AFTER);
        self.computers.retain(|_, (_, heard)| heard.elapsed() < FORGET_AFTER);
    }

    /// Addresses others can use: this computer's own mean nothing to them.
    pub fn reachable(&self) -> Vec<String> {
        self.known
            .iter()
            .filter(|(address, known)| {
                known.hello.is_some()
                    && known.heard.elapsed() < SILENT_AFTER
                    && device_ip(address).is_some_and(|ip| !ip.is_loopback())
            })
            .map(|(address, _)| address.clone())
            .take(MAX_KNOWN)
            .collect()
    }

    /// Everything to look at again.
    pub fn addresses(&self) -> Vec<String> {
        self.known.keys().cloned().collect()
    }

    /// `own` names this app's own node, which a look around also finds.
    pub fn entries(&self, own: (Address, u16)) -> Vec<Entry> {
        self.known
            .iter()
            .filter(|(_, known)| {
                let hello = known.hello.as_ref();
                !hello.is_some_and(|hello| (hello.address, hello.card.port) == own)
            })
            .filter(|(_, known)| known.hello.is_some() || known.added)
            .map(|(address, known)| entry_of(address, known))
            .chain(self.computers.values().filter(|(hello, _)| hello.address != own.0).map(entry_through_board))
            .collect()
    }
}

fn entry_through_board((hello, heard): &(Hello, Instant)) -> Entry {
    let known = Known { hello: Some(hello.clone()), via: None, heard: *heard, added: false };
    let reach = link_of(hello).map_or(Reach::Radio(None), |medium| Reach::Link(medium, None));
    Entry { reach, address: None, ..entry_of("", &known) }
}

fn entry_of(address: &str, known: &Known) -> Entry {
    let reach = device_ip(address).map_or(Reach::Internet, reach_of);
    let silent = known.heard.elapsed() > SILENT_AFTER || known.hello.is_none();
    let age_secs = (known.heard.elapsed() >= LIVE_WITHIN).then(|| known.heard.elapsed().as_secs());
    let work = |hello: &Hello| match (silent, hello.card.mining) {
        (true, _) => Work::Silent,
        (false, true) => Work::Mining(hello.card.hashrate),
        (false, false) => Work::Idle,
    };
    match &known.hello {
        Some(hello) => Entry {
            reach,
            address: Some(address.to_string()),
            via: known.via.clone(),
            device: hello.card.device.clone(),
            name: hello.card.name.clone(),
            identity: Some(Identity::of(hello.coin.clone())),
            tip: Some(hello.tip),
            height: Some(hello.height),
            work_done: Some(hello.work),
            build: hello.card.build.clone(),
            work: work(hello),
            keeps: hello.card.keeps,
            age_secs,
        },
        None => Entry {
            reach,
            address: Some(address.to_string()),
            via: known.via.clone(),
            device: String::new(),
            name: String::new(),
            identity: None,
            tip: None,
            height: None,
            work_done: None,
            build: String::new(),
            work: Work::Silent,
            keeps: Keeps::Unsaid,
            age_secs,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use olc_core::CoinConfig;
    use olc_node::Card;

    fn known(heard: Instant) -> Known {
        Known {
            hello: Some(Hello {
                coin: CoinConfig::olc(),
                tip: [0; 32],
                height: 1,
                work: 1,
                address: [0; 20],
                card: Card::default(),
            }),
            via: None,
            heard,
            added: false,
        }
    }

    #[test]
    fn made_up_names_stay_on_a_short_list_and_added_ones_stay() {
        let mut found = Found::default();
        found.added("10.0.0.1:7000");
        for number in 0..5_000 {
            let address = format!("10.1.{}.{}:7000", number / 256, number % 256);
            found.passed_on(&address, "a stranger");
        }
        assert!(found.known.len() <= MOST_KNOWN, "{} rows", found.known.len());
        assert!(found.known.contains_key("10.0.0.1:7000"));

        let hello = known(Instant::now()).hello.unwrap();
        for number in 0..5_000u32 {
            let mut made_up = hello.clone();
            made_up.address[..4].copy_from_slice(&number.to_le_bytes());
            made_up.card.device = if number % 2 == 0 { BOARD_KIND.into() } else { "computer".into() };
            found.board_said(made_up);
        }
        assert!(found.boards.len() <= MOST_ON_LINKS, "{} boards", found.boards.len());
        assert!(found.computers.len() <= MOST_ON_LINKS, "{} computers", found.computers.len());
    }

    #[test]
    fn a_computer_a_board_hears_gets_a_row_naming_its_link() {
        let mut found = Found::default();
        let mut hello = known(Instant::now()).hello.unwrap();
        hello.address = [7; 20];
        hello.card.device = "computer".into();
        hello.card.via = "bluetooth".into();
        found.board_said(hello);
        let rows = found.entries(([0; 20], 0));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].reach, Reach::Link(Medium::Bluetooth, None));
        assert!(found.entries(([7; 20], 0)).is_empty(), "this app's own node needs no second row");
    }

    #[test]
    fn a_row_from_an_old_answer_says_how_old_it_is() {
        let fresh = entry_of("127.0.0.1:7000", &known(Instant::now()));
        assert_eq!(fresh.age_secs, None);
        let old = Instant::now() - Duration::from_secs(44);
        assert_eq!(entry_of("127.0.0.1:7000", &known(old)).age_secs, Some(44));
    }
}
