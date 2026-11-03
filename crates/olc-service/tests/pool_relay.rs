//! Board zero has no cable and no control link here: the app hears it only as a node, passed on
//! by board one, the board on the cable. This runs the app's own pool with board zero as a worker
//! through board one. The app's fleet signs the work and sends it down board one's cable, board
//! one passes it on over the radio, board zero grinds it and its shares come back the same way,
//! credited to board zero. A board that does not trust this app's key stays silent: the pool
//! names it and turns its switch off.
//!
//! The fleet, the pool, each board's router and gate, and the rules for lent work are the real
//! ones; the firmware runs the same `olc_control::lend::handle`. Only the cable and the radio are
//! queues in this test, and board zero grinds on this computer.
use olc_board::control_key::ControlKey;
use olc_board::fleet::Fleet;
use olc_board::{Args, Extra};
use olc_control::lend::{handle, receipt, sign_lent, Handling, Heard};
use olc_control::{sign_beacon, Beacon, BoardId, Body, Forward, Gate, Packet, Router};
use olc_core::{BlockHeader, CoinConfig};
use olc_crypto::{address_of, Signer};
use olc_node::hello::Via;
use olc_node::{Card, Hello, Runner};
use olc_pool::meets;
use olc_service::core::{BlockState, Host, Plan, PoolCx, PoolOrder, PoolSeat, PoolSettings, PoolView, SwitchedOff, Who, LENT_ANSWER_WITHIN};
use olc_service::fleet::{BoardRow, Connection, FleetPort, LentHeard, LineError, StartError};
use olc_service::local::Supervisor;
use olc_service::settings::Paths;
use olc_wallet::format_address;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const SHARE_DIFFICULTY: u64 = 300;
const BLOCK_DIFFICULTY: u64 = 30_000;
const WAIT: Duration = Duration::from_secs(150);
/// Nonces board zero tries in one turn.
const BATCH: u32 = 3_000;
const BUILD: &str = "i19";

/// The link of this app that board one's cable is.
const CABLE_LINK: usize = 2;
/// How each board numbers the peer it hears another from.
const CABLE_ON_APP: u32 = 0x0200_0001;
const APP_ON_ONE: u32 = 1;
const ZERO_ON_ONE: u32 = 0x00AA_EF90;
const ONE_ON_ZERO: u32 = 0x00AA_E204;
const ONE: BoardId = [0xAC, 0x27, 0x6E, 0xAA, 0xE2, 0x04];
const ZERO: BoardId = [0xAC, 0x27, 0x6E, 0xAA, 0xEF, 0x90];

fn cable_link(_peer: u32) -> usize {
    CABLE_LINK
}

/// Board one: on the cable, it hears board zero over the radio and passes lent work on.
struct Relay {
    router: Router,
    gate: Gate,
    signer: Signer,
    address: [u8; 20],
    heard: Heard,
    inbox: VecDeque<(u32, Vec<u8>)>,
    passed: u32,
    beacon_at: Option<Instant>,
}

/// Board zero: radio only. It takes signed work and sends back what reached the difficulty.
struct Worker {
    router: Router,
    gate: Gate,
    address: [u8; 20],
    inbox: VecDeque<(u32, Vec<u8>)>,
    work: Option<(BlockHeader, u64, BoardId)>,
    taken: u32,
    sent: u32,
}

struct Desk {
    app: Fleet,
    one: Relay,
    zero: Worker,
}

/// The app's fleet as the pool sees it, with the two boards behind it.
struct Boards(Mutex<Desk>);

impl Desk {
    /// Puts what board one sends on its way: down the cable to the app, or over the radio to zero.
    fn one_sends(&mut self, forward: Forward, bytes: Vec<u8>) {
        let to_app = matches!(forward, Forward::Peer(APP_ON_ONE) | Forward::All | Forward::AllExcept(ZERO_ON_ONE));
        let to_zero = matches!(forward, Forward::Peer(ZERO_ON_ONE) | Forward::All | Forward::AllExcept(APP_ON_ONE));
        if to_zero {
            self.zero.inbox.push_back((ONE_ON_ZERO, bytes.clone()));
        }
        if to_app {
            self.app.hear(CABLE_ON_APP, &bytes);
        }
    }

    /// One turn of both boards and the cable between the app and board one, as the firmware's
    /// `serve_control`, `lent` and `serve_lent` do it.
    fn turn(&mut self) {
        while let Some((forward, bytes)) = self.app.take_outgoing() {
            // The app's one link is board one's cable: what it sends goes down it, unless the
            // packet came up it.
            if matches!(forward, Forward::Peer(CABLE_ON_APP) | Forward::All) {
                self.one.inbox.push_back((APP_ON_ONE, bytes));
            }
        }
        if self.one.beacon_at.is_none_or(|at| at.elapsed() >= Duration::from_secs(1)) {
            self.one.beacon_at = Some(Instant::now());
            let beacon = Beacon { name: "보드원".into(), address: self.one.address, build: BUILD.into(), mining: true, ..Beacon::default() };
            let packet = Packet { from: ONE, to: None, id: self.one.router.next_id(), hops: 0, body: Body::Beacon(sign_beacon(&self.one.signer, &ONE, beacon).unwrap()) };
            self.app.hear(CABLE_ON_APP, &packet.encode());
        }
        while let Some((peer, bytes)) = self.one.inbox.pop_front() {
            let Some(mut packet) = Packet::decode(&bytes) else { continue };
            let verdict = self.one.router.incoming(peer, &packet);
            if verdict.forward != Forward::Nowhere {
                packet.hops += 1;
                self.one_sends(verdict.forward, packet.encode());
                packet.hops -= 1;
            }
            if verdict.for_me {
                let handling = handle(&packet, &self.one.address, &self.one.gate, &self.one.heard);
                if let Some(body) = receipt(&packet, &handling, &self.one.address) {
                    let answer = Packet { from: ONE, to: Some(packet.from), id: self.one.router.next_id(), hops: 0, body };
                    let forward = self.one.router.outgoing(Some(packet.from));
                    self.one_sends(forward, answer.encode());
                }
                if let Handling::Pass(peer, passed) = handling {
                    assert_eq!(peer, ZERO_ON_ONE, "board one passes work to the peer board zero spoke from");
                    self.one.passed += 1;
                    self.one_sends(Forward::Peer(peer), passed.encode());
                }
            }
        }
        let zero = &mut self.zero;
        while let Some((peer, bytes)) = zero.inbox.pop_front() {
            let Some(packet) = Packet::decode(&bytes) else { continue };
            let verdict = zero.router.incoming(peer, &packet);
            assert_eq!(verdict.forward, Forward::Nowhere, "nothing for board zero goes further");
            if !verdict.for_me {
                continue;
            }
            let handling = handle(&packet, &zero.address, &zero.gate, &Heard::default());
            if let Some(body) = receipt(&packet, &handling, &zero.address) {
                let answer = Packet { from: ZERO, to: Some(packet.from), id: zero.router.next_id(), hops: 0, body };
                self.one.inbox.push_back((ZERO_ON_ONE, answer.encode()));
            }
            match handling {
                Handling::Take(Some((header, difficulty))) => {
                    zero.work = Some((BlockHeader::decode(&header).unwrap(), difficulty, packet.from));
                    zero.taken += 1;
                }
                Handling::Take(None) => zero.work = None,
                _ => {}
            }
        }
        let Some((header, difficulty, lender)) = &mut zero.work else { return };
        let mut found = Vec::new();
        for _ in 0..BATCH {
            if meets(header, *difficulty) {
                found.push(header.encode().to_vec());
            }
            header.nonce = header.nonce.wrapping_add(1);
        }
        if found.is_empty() {
            return;
        }
        zero.sent += found.len() as u32;
        let lender = *lender;
        let packet = Packet { from: ZERO, to: Some(lender), id: zero.router.next_id(), hops: 0, body: Body::Shares { worker: zero.address, headers: found } };
        assert_eq!(zero.router.outgoing(Some(lender)), Forward::Peer(ONE_ON_ZERO), "the shares go back the way the work came");
        self.one.inbox.push_back((ZERO_ON_ONE, packet.encode()));
    }
}

impl FleetPort for Boards {
    fn attach(&self, _runner: Arc<Mutex<Runner>>) {}
    fn pump(&self, _runner: &mut Runner) {}
    fn board_rows(&self) -> Vec<BoardRow> {
        let desk = self.0.lock().unwrap();
        let rows = desk.app.boards().into_iter();
        rows.map(|seen| BoardRow { on_cable: matches!(seen.connection, Connection::Direct { .. }), seen, through: None, job: None }).collect()
    }
    fn new_boards(&self) -> Vec<String> {
        Vec::new()
    }
    fn job(&self, _key: &str) -> Option<String> {
        None
    }
    fn run_line(&self, _board: BoardId, _line: &str) -> Result<(), LineError> {
        Err(LineError::NoAnswer)
    }
    fn ask(&self, _board: BoardId, _line: &str) -> Result<Option<String>, LineError> {
        Err(LineError::NoAnswer)
    }
    fn lend(&self, relay: BoardId, worker: [u8; 20], work: Option<(Vec<u8>, u64)>) -> bool {
        self.0.lock().unwrap().app.lend(relay, worker, work).is_ok()
    }
    fn take_lent_shares(&self, worker: &[u8; 20]) -> Vec<Vec<u8>> {
        self.0.lock().unwrap().app.take_shares(worker)
    }
    fn lent_heard(&self, worker: &[u8; 20]) -> LentHeard {
        self.0.lock().unwrap().app.lent_heard(worker)
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

fn coin() -> CoinConfig {
    let mut coin = CoinConfig::olc();
    coin.name = "PoolRelay".into();
    coin.ticker = "PRL".into();
    coin.initial_difficulty = BLOCK_DIFFICULTY;
    coin.rules.min_difficulty = 1;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis();
    coin.genesis_time_ms = u64::try_from(now).unwrap() - 5_000;
    coin
}

fn fresh_folder(name: &str) -> PathBuf {
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    folder
}

fn host(folder: &Path) -> Host {
    let plan = Plan {
        coin: coin(),
        folder: folder.join("chain"),
        wallet_folder: folder.join("wallet"),
        bind_host: "127.0.0.1".into(),
        port: 0,
        port_tries: 1,
        peers: Vec::new(),
        cap: 0,
        mining: false,
        card: Card::default(),
        storage: None,
        shares_wallet: false,
    };
    Host::start(plan, None).unwrap()
}

/// The two boards; each trusts this app's control key, or another computer's.
fn desk(folder: &Path, one_trusts: bool, zero_trusts: bool) -> Desk {
    let key = ControlKey::load_or_create(folder).unwrap();
    let ours = key.public().ok();
    let theirs = olc_crypto::public_key(&[0x44; 32]);
    let trusted = |trusts: bool| if trusts { ours } else { theirs };
    let app = Fleet::new(key, cable_link).unwrap();
    let one_signer = Signer::new(&[0x11; 32]).unwrap();
    let zero_signer = Signer::new(&[0x22; 32]).unwrap();
    let mut one = Relay {
        router: Router::new(ONE, 100),
        gate: Gate::new(ONE, trusted(one_trusts), 0),
        address: address_of(one_signer.public_key()),
        signer: one_signer,
        heard: Heard::default(),
        inbox: VecDeque::new(),
        passed: 0,
        beacon_at: None,
    };
    let zero = Worker {
        router: Router::new(ZERO, 200),
        gate: Gate::new(ZERO, trusted(zero_trusts), 0),
        address: address_of(zero_signer.public_key()),
        inbox: VecDeque::new(),
        work: None,
        taken: 0,
        sent: 0,
    };
    // Board zero answered board one's question of who is around, as `pass_hellos` hears it.
    one.heard.heard(zero.address, ZERO_ON_ONE);
    Desk { app, one, zero }
}

/// Board zero's answer, as board one passed it to the app over its cable.
fn hello_of(zero: [u8; 20]) -> Hello {
    let card = Card { name: "보드제로".into(), device: "esp32-s3".into(), build: BUILD.into(), mining: true, hashrate: 9_000, via: Via("espnow".into()), ..Card::default() };
    Hello { coin: coin(), tip: [0; 32], height: 0, work: 0, address: zero, card }
}

#[test]
fn board_zero_works_for_the_pool_through_board_one() {
    let folder = fresh_folder("pool_relay");
    let host = host(&folder);
    let mut local = Supervisor::with_program(Paths { root: folder.clone() }, "olc-core".into());
    let boards = Arc::new(Boards(Mutex::new(desk(&folder, true, true))));
    let port: Arc<dyn FleetPort> = boards.clone();
    let zero = boards.0.lock().unwrap().zero.address;
    let zero_hello = hello_of(zero);
    let hellos = [(&zero_hello, Some(CABLE_LINK))];
    let mut settings = PoolSettings { port: 0, fixed_difficulty: SHARE_DIFFICULTY, ..PoolSettings::default() };
    let mut seat = PoolSeat::default();
    let started = Instant::now();
    let mut ordered = false;
    let mut forged = false;
    let view = loop {
        boards.0.lock().unwrap().turn();
        let mut cx = PoolCx { host: &host, local: &mut local, fleet: Some(&port), specs: &[], hellos: &hellos, settings: &mut settings };
        seat.step(&mut cx);
        let view = seat.view();
        if !ordered {
            // Board zero is offered once board one's beacon says it runs a build that passes work on.
            if let Some(choice) = view.choices.iter().find(|choice| choice.label == "보드제로" && choice.who.is_some()) {
                assert_eq!(choice.who, Some(Who::Lent(zero)));
                assert_eq!((choice.through.as_deref(), choice.why_not), (Some("보드원"), None));
                let chosen = PoolSettings { port: 0, fixed_difficulty: SHARE_DIFFICULTY, workers: vec![Who::Lent(zero)], ..PoolSettings::default() };
                assert!(seat.order(PoolOrder::Start(chosen), &mut cx), "{:?}", view.error);
                ordered = true;
            }
        }
        // Someone in radio reach hands board zero work of their own through board one.
        let working = boards.0.lock().unwrap().zero.work.is_some();
        if working && !forged {
            forged = true;
            let stranger = sign_lent(&[0x33; 32], zero, Some((vec![0; 88], 1))).unwrap();
            let packet = Packet { from: [9; 6], to: Some(ONE), id: 1, hops: 0, body: Body::Lend(stranger) };
            let mut desk = boards.0.lock().unwrap();
            let passed = desk.one.passed;
            desk.one.inbox.push_back((APP_ON_ONE, packet.encode()));
            desk.turn();
            assert_eq!(desk.one.passed, passed, "board one passed on work nobody here signed");
            assert_ne!(desk.zero.work.as_ref().map(|(_, difficulty, _)| *difficulty), Some(1), "board zero took work nobody here signed");
        }
        assert!(started.elapsed() < WAIT, "{view:#?}");
        let zero_row = view.workers.iter().find(|row| row.here == Some(Who::Lent(zero)));
        if zero_row.is_some_and(|row| row.paid > 0) && view.found.iter().any(|row| row.state == BlockState::Paid) {
            break view;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let row = view.workers.iter().find(|row| row.here == Some(Who::Lent(zero))).unwrap();
    assert_eq!(row.name, format!("{}.보드제로", format_address(&zero)), "credited to board zero's own address");
    assert!(row.accepted > 0, "{row:?}");
    let others: Vec<_> = view.workers.iter().filter(|other| other.name != row.name).collect();
    assert!(others.is_empty(), "only board zero hashed, so it found the block: {others:?}");
    {
        let desk = boards.0.lock().unwrap();
        assert!(desk.one.passed > 0 && desk.zero.taken > 0 && desk.zero.sent > 0);
        let heard = desk.app.lent_heard(&zero);
        assert!(heard.passed.is_some() && heard.taken.is_some(), "both boards said so: {heard:?}");
        assert!(view.switched_off.is_empty(), "{:?}", view.switched_off);
        println!("board one passed {} works, board zero took {} and sent {} shares; the pool took {}", desk.one.passed, desk.zero.taken, desk.zero.sent, row.accepted);
    }

    // Stopping the pool stops board zero's lent work, through board one again.
    let mut cx = PoolCx { host: &host, local: &mut local, fleet: Some(&port), specs: &[], hellos: &hellos, settings: &mut settings };
    assert!(seat.order(PoolOrder::Stop, &mut cx));
    for _ in 0..3 {
        boards.0.lock().unwrap().turn();
    }
    assert!(boards.0.lock().unwrap().zero.work.is_none(), "board zero mines for itself again");
}

/// Runs the pool with board zero chosen until the pool gives up on it. What the page has then,
/// the settings left, how long after Start, and what board one passed on and board zero took.
fn until_switched_off(name: &str, one_trusts: bool, zero_trusts: bool) -> (SwitchedOff, PoolView, PoolSettings, Duration, (u32, u32)) {
    let folder = fresh_folder(name);
    let host = host(&folder);
    let mut local = Supervisor::with_program(Paths { root: folder.clone() }, "olc-core".into());
    let boards = Arc::new(Boards(Mutex::new(desk(&folder, one_trusts, zero_trusts))));
    let port: Arc<dyn FleetPort> = boards.clone();
    let zero = boards.0.lock().unwrap().zero.address;
    let zero_hello = hello_of(zero);
    let hellos = [(&zero_hello, Some(CABLE_LINK))];
    let mut settings = PoolSettings::default();
    let mut seat = PoolSeat::default();
    let began = Instant::now();
    let mut started: Option<Instant> = None;
    loop {
        boards.0.lock().unwrap().turn();
        let mut cx = PoolCx { host: &host, local: &mut local, fleet: Some(&port), specs: &[], hellos: &hellos, settings: &mut settings };
        let changed = seat.step(&mut cx);
        let view = seat.view();
        // Nothing tells the app before Start that a board does not trust it.
        let offered = view.choices.iter().any(|choice| choice.who == Some(Who::Lent(zero)) && choice.why_not.is_none());
        if started.is_none() && offered {
            let chosen = PoolSettings { port: 0, fixed_difficulty: SHARE_DIFFICULTY, workers: vec![Who::Lent(zero)], ..PoolSettings::default() };
            assert!(seat.order(PoolOrder::Start(chosen), &mut cx), "{:?}", view.error);
            started = Some(Instant::now());
        }
        if let Some(off) = view.switched_off.first() {
            assert!(changed, "the turn that switched it off says the settings changed");
            let desk = boards.0.lock().unwrap();
            return (off.clone(), view.clone(), settings.clone(), started.unwrap().elapsed(), (desk.one.passed, desk.zero.taken));
        }
        assert!(began.elapsed() < WAIT, "{view:#?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_board_that_does_not_trust_this_app_is_named_and_switched_off() {
    let (off, view, settings, after, (passed, taken)) = until_switched_off("pool_relay_untrusted_zero", true, false);
    assert_eq!((off.why, off.relay.as_str()), ("pool.lent_untrusted", "보드원"));
    assert!(matches!(off.who, Who::Lent(_)));
    assert!(after >= LENT_ANSWER_WITHIN && after < LENT_ANSWER_WITHIN + Duration::from_secs(3), "after {after:?}");
    // Board one passed the work on each time it went out again; board zero never took it.
    assert!(passed >= 3, "passed {passed}");
    assert_eq!(taken, 0);
    assert!(!settings.workers.contains(&off.who), "its switch is off: {:?}", settings.workers);
    assert!(view.link_of(off.who).is_none(), "nothing is lent to it any more");
    // Its row in the pool's table is still its own, not a stranger's.
    for row in view.workers.iter().filter(|row| row.name == off.user) {
        assert_eq!(row.here, Some(off.who));
    }
    println!("board zero was switched off {after:?} after Start; board one passed {passed} works on");
}

#[test]
fn a_relay_that_does_not_pass_the_work_on_is_named() {
    let (off, view, settings, after, (passed, taken)) = until_switched_off("pool_relay_untrusted_one", false, true);
    assert_eq!((off.why, off.relay.as_str()), ("pool.relay_untrusted", "보드원"));
    assert!(after >= LENT_ANSWER_WITHIN && after < LENT_ANSWER_WITHIN + Duration::from_secs(3), "after {after:?}");
    assert_eq!((passed, taken), (0, 0));
    assert!(!settings.workers.contains(&off.who));
    assert!(view.link_of(off.who).is_none());
}
