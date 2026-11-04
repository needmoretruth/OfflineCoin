//! The owner's three boards as the app's fleet sees them, with no board touched: boards one and two
//! on the USB cables, and board zero heard only by radio, its hello passed up board one's cable.
//! Each board grinds the pool work it is handed on this computer, a few thousand nonces a turn,
//! and hands back the shares that reach the difficulty, as the firmware's `work` lines and lent
//! work do. Board two can run an older build until the scene updates it.
use crate::core::meets;
use olc_control::{Beacon, BoardId};
use olc_core::{BlockHeader, CoinConfig};
use olc_node::hello::Via;
use olc_node::message::Message;
use olc_node::network::{stamp, OPEN};
use olc_node::{Card, Hello, Link, Medium, Runner, Target};
use olc_service::fleet::{Args, BoardRow, Connection, Extra, FleetPort, LentHeard, LineError, Seen, StartError};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub(super) const ONE: BoardId = [0xAC, 0x27, 0x6E, 0xAA, 0xE2, 0x04];
pub(super) const TWO: BoardId = [0xAC, 0x27, 0x6E, 0xAA, 0xEF, 0x78];
pub(super) const ZERO: [u8; 20] = [0x5A; 20];
/// Nonces each board tries in one turn, a turn every 20 ms.
const BATCH: u32 = 2_000;
const NEW_BUILD: &str = "i19";
const OLD_BUILD: &str = "i12";

fn address_of(id: &BoardId) -> [u8; 20] {
    let mut address = [0x11; 20];
    address[..6].copy_from_slice(id);
    address
}

/// What one board grinds: the header it was handed, the difficulty, and what it found.
#[derive(Default)]
struct Grind {
    work: Option<(BlockHeader, u64)>,
    found: Vec<BlockHeader>,
    taken: Option<Instant>,
    passed: Option<Instant>,
}

#[derive(Default)]
struct Desk {
    /// Boards one and two by id, board zero by its address.
    cable: BTreeMap<BoardId, Grind>,
    lent: Grind,
    /// The index of board one's cable among the runner's links, once attached.
    link: Option<usize>,
    two_build: String,
}

/// The fleet the app is given in place of the real one.
pub(super) struct Boards {
    desk: Mutex<Desk>,
    coin: CoinConfig,
    stop: Arc<AtomicBool>,
}

impl Boards {
    pub(super) fn new(coin: &CoinConfig, two_updated: bool) -> Arc<Self> {
        let desk = Desk { two_build: if two_updated { NEW_BUILD } else { OLD_BUILD }.into(), ..Desk::default() };
        let boards = Arc::new(Self { desk: Mutex::new(desk), coin: coin.clone(), stop: Arc::new(AtomicBool::new(false)) });
        let grinding = boards.clone();
        thread::spawn(move || {
            while !grinding.stop.load(Ordering::Relaxed) {
                grinding.turn();
                thread::sleep(Duration::from_millis(20));
            }
        });
        boards
    }

    /// Board two now runs the build that takes pool work, as after an update.
    pub(super) fn update_two(&self) {
        if let Ok(mut desk) = self.desk.lock() {
            desk.two_build = NEW_BUILD.into();
        }
    }

    pub(super) fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    fn turn(&self) {
        let Ok(mut desk) = self.desk.lock() else { return };
        let Desk { cable, lent, .. } = &mut *desk;
        for grind in cable.values_mut().chain(std::iter::once(lent)) {
            let Some((header, difficulty)) = &mut grind.work else { continue };
            for _ in 0..BATCH {
                if meets(header, *difficulty) {
                    grind.found.push(*header);
                }
                header.nonce = header.nonce.wrapping_add(1);
            }
        }
    }

    /// Board zero's hello, as board one passes it up the cable.
    fn zero_hello(&self) -> Vec<u8> {
        let card = Card { name: "보드제로".into(), device: "esp32-s3".into(), build: NEW_BUILD.into(), mining: true, hashrate: 9_000, via: Via("espnow".into()), ..Card::default() };
        let hello = Hello { coin: self.coin.clone(), tip: [0; 32], height: 0, work: 0, address: ZERO, card };
        stamp(OPEN, &Message::Hello(Box::new(hello)).encode())
    }

    fn row(&self, id: BoardId, name: &str, build: &str, link: usize) -> BoardRow {
        let beacon = Beacon { name: name.into(), address: address_of(&id), build: build.into(), mining: true, hashrate: 9_000, limit: 10_000, ..Beacon::default() };
        let seen = Seen { id, beacon, connection: Connection::Direct { link }, online: true, silent_for: Duration::ZERO, old_format: false };
        BoardRow { seen, on_cable: true, through: None, job: None }
    }
}

/// Board one's cable as the node hears it: board zero's hello every two seconds.
struct Cable {
    hello: Vec<u8>,
    last: Option<Instant>,
}

impl Link for Cable {
    fn medium(&self) -> Medium {
        Medium::Wire
    }

    fn send(&mut self, _target: Target, _message: &[u8]) {}

    fn receive(&mut self) -> Option<(u32, Vec<u8>)> {
        let due = self.last.is_none_or(|last| last.elapsed() >= Duration::from_secs(2));
        due.then(|| {
            self.last = Some(Instant::now());
            (1, self.hello.clone())
        })
    }
}

impl FleetPort for Boards {
    fn attach(&self, runner: Arc<Mutex<Runner>>) {
        let cable = Cable { hello: self.zero_hello(), last: None };
        let link = runner.lock().map(|mut runner| runner.add_link(Box::new(cable))).ok();
        if let Ok(mut desk) = self.desk.lock() {
            desk.link = link;
        }
    }

    fn pump(&self, _runner: &mut Runner) {}

    fn board_rows(&self) -> Vec<BoardRow> {
        let Ok(desk) = self.desk.lock() else { return Vec::new() };
        let Some(link) = desk.link else { return Vec::new() };
        vec![self.row(ONE, "보드원", NEW_BUILD, link), self.row(TWO, "보드투", &desk.two_build, link + 100)]
    }

    fn new_boards(&self) -> Vec<String> {
        Vec::new()
    }

    fn job(&self, _key: &str) -> Option<String> {
        None
    }

    fn run_line(&self, board: BoardId, line: &str) -> Result<(), LineError> {
        let words: Vec<&str> = line.split_whitespace().collect();
        let mut desk = self.desk.lock().map_err(|_| LineError::NoAnswer)?;
        let grind = desk.cable.entry(board).or_default();
        match words.as_slice() {
            ["work", "stop"] => grind.work = None,
            ["work", header, difficulty] => {
                let header = unhex(header).and_then(|bytes| BlockHeader::decode(&bytes));
                grind.work = header.zip(difficulty.parse().ok());
            }
            _ => {}
        }
        Ok(())
    }

    fn ask(&self, board: BoardId, line: &str) -> Result<Option<String>, LineError> {
        if line != "work shares" {
            return Ok(None);
        }
        let mut desk = self.desk.lock().map_err(|_| LineError::NoAnswer)?;
        let found = desk.cable.get_mut(&board).map(|grind| std::mem::take(&mut grind.found)).unwrap_or_default();
        let words: Vec<String> = found.iter().map(|header| hex(&header.encode())).collect();
        Ok(Some(format!("shares {}", words.join(" "))))
    }

    fn lend(&self, relay: BoardId, worker: [u8; 20], work: Option<(Vec<u8>, u64)>) -> bool {
        let Ok(mut desk) = self.desk.lock() else { return false };
        if relay != ONE || worker != ZERO {
            return false;
        }
        let now = Instant::now();
        desk.lent.passed = Some(now);
        desk.lent.taken = Some(now);
        desk.lent.work = work.and_then(|(header, difficulty)| BlockHeader::decode(&header).map(|header| (header, difficulty)));
        true
    }

    fn take_lent_shares(&self, worker: &[u8; 20]) -> Vec<Vec<u8>> {
        let Ok(mut desk) = self.desk.lock() else { return Vec::new() };
        if *worker != ZERO {
            return Vec::new();
        }
        std::mem::take(&mut desk.lent.found).iter().map(|header| header.encode().to_vec()).collect()
    }

    fn lent_heard(&self, worker: &[u8; 20]) -> LentHeard {
        let Ok(desk) = self.desk.lock() else { return LentHeard::default() };
        match *worker == ZERO {
            true => LentHeard { passed: desk.lent.passed, taken: desk.lent.taken },
            false => LentHeard::default(),
        }
    }

    fn start_update(&self, _board: BoardId) {}

    fn start_install(&self, _path: &str, _name: &str) -> bool {
        false
    }

    fn start_extra(&self, _extra: &'static dyn Extra, _boards: &[BoardId], _args: Args) -> Result<(), StartError> {
        Ok(())
    }

    fn run_log(&self) -> (Vec<String>, bool) {
        (Vec::new(), false)
    }

    fn stop_extra(&self) {}
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    let pairs = text.as_bytes().chunks(2);
    pairs.map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()).collect()
}
