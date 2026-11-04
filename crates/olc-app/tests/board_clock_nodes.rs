//! The owner's scene of 5 November 2026: boards mined half a day with the app closed, then the
//! app opened on their chain. A board has no clock of the date, so it counted its block times from
//! its chain's tip, which on a fresh chain is the coin's genesis time, and every block it made
//! read about a day old. The app, with the date, then mined its first block that late and took the
//! lowest difficulty at once.
//!
//! The boards' half day is simulated: an engine with a board's clock (its own uptime, no date)
//! mines 1,500 blocks 30 seconds apart into a chain folder, as a board keeps one. A real node
//! process then serves that folder over the same messages a board answers, and the app's own node
//! (the host OLC Core runs, with the date) joins it with mining switched on.
use olc_app::core::{unix_ms, BlockRow, Host, NodeView, Plan};
use olc_control::dated::{handle, read_record, record, sign_dated, Handling, Keeping, SAVE_EVERY_MS};
use olc_control::{BoardId, Body, Gate, Packet};
use olc_core::{CoinConfig, Plugins};
use olc_pow::ProofOfWork;
use olc_node::{Card, Engine};
use olc_store::{coin_file, open_chain};
use olc_wallet::Wallet;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BLOCKS: u64 = 1_500;
const BLOCK_MS: u64 = 30_000;
/// How long after the coin's genesis the boards began this chain on the owner's network.
const BOARDS_BEGAN_AFTER_GENESIS_MS: u64 = 94_700_000;
const FLOOR: u64 = 40;
const DIFFICULTY: u64 = 400;
const WAIT: Duration = Duration::from_secs(120);
/// The app's control key, made for these tests.
const APP_KEY: [u8; 32] = [0x5C; 32];
/// How long a board takes to start, by its own clock.
const BOOT_MS: u64 = 5_000;
const CABLE_BOARD: BoardId = [0xAC, 0x27, 0x6E, 0xAA, 0xE2, 0x04];
const RADIO_BOARD: BoardId = [0xAC, 0x27, 0x6E, 0xAA, 0xEF, 0x90];
/// A board's block may read this far from the app's time: the clock is kept to the
/// millisecond, and a block carries whole seconds.
const CLOSE_MS: u64 = 2_000;

/// A coin like OLC, born before the boards began, cheap enough to mine 1,500 blocks in a test.
fn coin(run_ms: u64) -> CoinConfig {
    let mut coin = CoinConfig::olc();
    coin.name = "BoardClock".into();
    coin.ticker = "BCK".into();
    coin.initial_difficulty = DIFFICULTY;
    coin.rules.min_difficulty = FLOOR;
    coin.rules.floor_from_height = 0;
    coin.genesis_time_ms = unix_ms() - run_ms - BOARDS_BEGAN_AFTER_GENESIS_MS;
    coin
}

fn plugins() -> Plugins {
    Plugins { consensus: Box::new(ProofOfWork), languages: olc_lang::all() }
}

fn fresh_folder(name: &str) -> PathBuf {
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    folder
}

/// What a board runs, as the firmware runs it: the engine on its chain folder, its clock its own
/// uptime, the gate that checks the app's signature, and the flash record of the date it knew.
struct Board {
    engine: Engine,
    gate: Gate,
    uptime_ms: u64,
    keeping: Keeping,
    /// The date record in its flash, which outlives a restart.
    saved: Option<[u8; 16]>,
    passed: u64,
}

impl Board {
    /// Starts after `boot_ms`, from the date in `saved` when there is one, as the firmware's
    /// `main` does.
    fn start(folder: &Path, coin: &CoinConfig, id: BoardId, saved: Option<[u8; 16]>) -> Self {
        let chain = open_chain(&folder.join("chain"), coin.clone(), plugins(), None).unwrap();
        let mut engine = Engine::on_chain(chain, Wallet::from_secret([0x0B; 32]).unwrap());
        engine.set_merging(false);
        let gate = Gate::new(id, olc_crypto::public_key(&APP_KEY), 0);
        let mut board = Self { engine, gate, uptime_ms: BOOT_MS, keeping: Keeping::default(), saved, passed: 0 };
        if let Some(date) = saved.as_ref().and_then(read_record) {
            board.engine.take_date(date, BOOT_MS);
            board.keeping.learned(BOOT_MS, true);
        }
        board.engine.tick(BOOT_MS);
        board
    }

    /// A control packet that reached the board, as the firmware's `dated` takes it: what it
    /// passes on by radio, if anything.
    fn hear(&mut self, packet: &Packet) -> Option<Box<Packet>> {
        let Handling::Take { unix_ms, pass } = handle(packet, &self.gate, self.passed) else {
            return None;
        };
        self.engine.take_date(unix_ms, self.uptime_ms);
        self.keeping.learned(self.uptime_ms, false);
        self.passed = self.passed.max(pass.as_ref().map_or(0, |_| unix_ms));
        pass
    }

    /// How long the board's steady hash rate takes for the next block: 30 seconds at the coin's
    /// starting difficulty, less on an easier one, as a board's fixed rate does on average.
    fn next_after(&self) -> u64 {
        let difficulty = self.engine.chain().tip_header().difficulty;
        (BLOCK_MS * difficulty / DIFFICULTY).max(1_000)
    }

    /// Finds the next block after its hash rate's time, then saves the date when it is due, as
    /// the firmware's `keep_date` does.
    fn mine_one(&mut self) {
        self.uptime_ms += self.next_after();
        self.mine_now();
    }

    fn mine_now(&mut self) {
        self.engine.tick(self.uptime_ms);
        while !self.engine.mine(10_000, self.uptime_ms) {}
        while self.engine.take_outgoing().is_some() {}
        while self.engine.take_event().is_some() {}
        if self.keeping.due(self.uptime_ms) {
            self.saved = Some(record(self.engine.clock_at(self.uptime_ms)));
        }
    }

    fn mine_to(&mut self, height: u64) {
        while self.engine.chain().height() < height {
            self.mine_one();
        }
        self.engine.chain_mut().save_now();
    }

    /// Mines on while its next block would still come before the true time, read off this
    /// computer's clock with the board's uptime starting at `true_zero`.
    fn mine_until_now(&mut self, true_zero: u64) {
        while true_zero + self.uptime_ms + self.next_after() <= unix_ms() {
            self.mine_one();
        }
        self.engine.chain_mut().save_now();
    }

    fn tip_time(&self) -> u64 {
        self.engine.chain().tip_header().time_ms
    }
}

/// The app's signed time at `unix_ms`, sent down the cable to `board`.
fn dated(board: BoardId, unix_ms: u64) -> Packet {
    let body = Body::Dated(sign_dated(&APP_KEY, unix_ms).unwrap());
    Packet { from: [0xC0; 6], to: Some(board), id: 1, hops: 0, body }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// The board's chain served by a node process of its own, which does not mine.
struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn serve(folder: &Path, coin: &CoinConfig, port: u16) -> Server {
    let coin_path = folder.join("coin.json");
    std::fs::write(&coin_path, coin_file::to_json(coin).unwrap()).unwrap();
    let words = [
        "node".to_string(),
        "--data".into(),
        folder.join("chain").display().to_string(),
        "--wallet".into(),
        folder.join("wallet").display().to_string(),
        "--coin".into(),
        coin_path.display().to_string(),
        "--bind".into(),
        "127.0.0.1".into(),
        "--port".into(),
        port.to_string(),
    ];
    let child = Command::new(env!("CARGO_BIN_EXE_olc-core"))
        .args(words)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    Server(child)
}

/// The app opening for the first time on this computer, mining from the start, as the owner had
/// it with a cap of 2,000 hashes a second.
fn app(folder: &Path, coin: &CoinConfig, peer: String) -> Host {
    Host::start(
        Plan {
            coin: coin.clone(),
            folder: folder.join("app-chain"),
            wallet_folder: folder.join("app-wallet"),
            bind_host: "127.0.0.1".into(),
            port: 0,
            port_tries: 1,
            peers: vec![peer],
            cap: 0,
            mining: true,
            card: Card::default(),
            storage: None,
            shares_wallet: false,
        },
        None,
    )
    .unwrap()
}

struct Joined {
    /// From the app's start until it held every block the board had.
    took: Duration,
    /// Blocks the app mined before it held the board's chain. Bitcoin Core refuses to mine while
    /// it downloads the chain: a block made then is made on a tip the network left long ago.
    mined_early: u64,
    /// The app's first block of its own.
    first: BlockRow,
    /// The board's last block, as the app shows it.
    boards_last: BlockRow,
}

fn row(view: &NodeView, height: u64) -> BlockRow {
    let found = view.blocks.iter().find(|row| row.height == height);
    found.cloned().unwrap_or_else(|| panic!("no row for block {height}"))
}

/// Starts the app against the board's chain, and waits for it to take the chain and mine once.
fn join(folder: &Path, coin: &CoinConfig, boards: u64) -> Joined {
    let port = free_port();
    let _server = serve(folder, coin, port);
    let host = app(folder, coin, format!("127.0.0.1:{port}"));
    let started = Instant::now();
    let mut took = None;
    let mut mined_early = 0;
    let mut last_height = (Duration::ZERO, 0);
    loop {
        let view = host.view();
        assert!(started.elapsed() < WAIT, "height {} of {boards}", view.height);
        if took.is_none() && view.height >= boards {
            took = Some(started.elapsed());
        }
        if took.is_none() {
            mined_early = mined_early.max(view.blocks_mined);
            if view.height != last_height.1 {
                if std::env::var_os("OLC_SYNC_TRACE").is_some() {
                    eprintln!("{:>6} ms height {} peers {}", started.elapsed().as_millis(), view.height, view.peers);
                }
                last_height = (started.elapsed(), view.height);
            }
        }
        if let Some(took) = took.filter(|_| view.blocks_mined > 0 && view.height > boards) {
            let first = row(&view, boards + 1);
            let boards_last = row(&view, boards);
            host.set_mining(false);
            return Joined { took, mined_early, first, boards_last };
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The cause, as it was on the owner's chain: a board that was never given the date stamps a fresh
/// chain from its genesis time, so its blocks lag the date by as long as the coin waited for its
/// first block, and the app's first block, on a tip that old, takes the floor at once.
#[test]
fn an_undated_board_lags_by_the_wait_for_its_first_block_and_the_app_then_mines_at_the_floor() {
    let run_ms = BLOCKS * BLOCK_MS;
    let coin = coin(run_ms);
    let folder = fresh_folder("board_clock_undated");
    let mut board = Board::start(&folder, &coin, CABLE_BOARD, None);
    let began = unix_ms();
    board.mine_to(BLOCKS);
    let stamped = board.tip_time();
    assert_eq!(board.saved, None, "a board with no date saves none");
    drop(board);
    let lag = began - stamped;
    println!("the board's last block lags the date by {:.1} h", lag as f64 / 3_600_000.0);
    assert!(lag.abs_diff(BOARDS_BEGAN_AFTER_GENESIS_MS) < 60_000, "lag {lag} ms");

    let joined = join(&folder, &coin, BLOCKS);
    println!("the app took {BLOCKS} blocks in {:?}", joined.took);
    assert_eq!(joined.mined_early, 0, "the app mined before it took the chain");
    // A chain already stamped this late leaves the rules no choice: the next block on time is
    // a day late, and a late block may carry the floor. New chains are no longer stamped so.
    assert_eq!(joined.first.difficulty, FLOOR, "{:?}", joined.first);
    assert!(joined.first.time_ms - joined.boards_last.time_ms > BOARDS_BEGAN_AFTER_GENESIS_MS - 60_000);
}

/// The scene the owner asked for: the app gave the boards the date when they began and then
/// closed; the board on its cable mined half a day and restarted once on the way; the app opens.
/// It takes the chain in seconds, mines nothing before, every block reads its true time, and its
/// first block is an ordinary one.
#[test]
fn boards_given_the_date_stamp_true_times_and_the_apps_first_block_is_ordinary() {
    let run_ms = BLOCKS * BLOCK_MS;
    let coin = coin(run_ms);
    let folder = fresh_folder("board_clock_dated");
    // The true time at the moment the board was switched on, by this computer's clock.
    let true_zero = unix_ms() - run_ms - BOOT_MS;
    let mut board = Board::start(&folder, &coin, CABLE_BOARD, None);
    board.hear(&dated(CABLE_BOARD, true_zero + BOOT_MS));
    board.mine_to(BLOCKS / 2);
    let lag = (true_zero + board.uptime_ms).abs_diff(board.tip_time());
    assert!(lag < CLOSE_MS, "the dated board's block is {lag} ms off");
    // A power cut, and the board starts again from the date it saved, with no app to give one.
    let true_zero = true_zero + board.uptime_ms;
    let saved = board.saved;
    drop(board);
    let mut board = Board::start(&folder, &coin, CABLE_BOARD, saved);
    board.mine_until_now(true_zero);
    let lag = (true_zero + board.uptime_ms).saturating_sub(board.tip_time());
    println!("after the restart the board's blocks lag the date by {} s", lag / 1_000);
    assert!(lag <= SAVE_EVERY_MS + BOOT_MS + CLOSE_MS, "lag {lag} ms");
    // The app opens and gives the board on its cable the date, and the board finds a block.
    board.hear(&dated(CABLE_BOARD, unix_ms()));
    board.mine_now();
    board.engine.chain_mut().save_now();
    assert!(board.tip_time().abs_diff(unix_ms()) < CLOSE_MS, "{}", board.tip_time());
    let boards = board.engine.chain().height();
    drop(board);

    let joined = join(&folder, &coin, boards);
    println!("the app took {boards} blocks in {:?}", joined.took);
    // About 4 seconds on a quiet machine in a debug build; the bound leaves room for a busy one.
    assert!(joined.took < Duration::from_secs(20), "{:?}", joined.took);
    assert_eq!(joined.mined_early, 0, "the app mined before it took the chain");
    // "3분 전", not "26시간 전": the board's last block reads seconds before the app's first,
    // well inside the 90 seconds after which a block may take the floor.
    let gap = joined.first.time_ms - joined.boards_last.time_ms;
    println!("the app's first block came {} s after the board's last, at difficulty {}", gap / 1_000, joined.first.difficulty);
    assert!(gap < 2 * BLOCK_MS, "gap {gap} ms");
    assert!(joined.first.difficulty > DIFFICULTY / 2, "{:?}", joined.first);
}

/// The date reaches the board on the cable and, through it, a board heard only by radio. A time
/// signed by another key, or an old copy sent again, moves no clock.
#[test]
fn a_board_takes_the_date_by_cable_and_through_another_and_only_from_the_apps_key() {
    let coin = coin(0);
    let now = unix_ms();
    let mut cable = Board::start(&fresh_folder("board_clock_cable"), &coin, CABLE_BOARD, None);
    let mut radio = Board::start(&fresh_folder("board_clock_radio"), &coin, RADIO_BOARD, None);
    let mut other = Board::start(&fresh_folder("board_clock_other"), &coin, RADIO_BOARD, None);
    let passed = cable.hear(&dated(CABLE_BOARD, now)).expect("the board on the cable passes it on");
    assert!(radio.hear(&passed).is_none(), "one hop only");
    let mut forged = dated(RADIO_BOARD, now);
    forged.body = Body::Dated(sign_dated(&[0x66; 32], now).unwrap());
    assert!(other.hear(&forged).is_none());
    for board in [&mut cable, &mut radio, &mut other] {
        board.mine_to(1);
    }
    let on_time = now - BOOT_MS + BOOT_MS + BLOCK_MS;
    assert!(cable.tip_time().abs_diff(on_time) < CLOSE_MS, "{} {on_time}", cable.tip_time());
    assert!(radio.tip_time().abs_diff(on_time) < CLOSE_MS, "{} {on_time}", radio.tip_time());
    let behind = on_time - other.tip_time();
    assert!(behind > BOARDS_BEGAN_AFTER_GENESIS_MS - 60_000, "a forged time moved the clock: {behind}");
    // The same time sent again, or an older one, changes nothing and crosses no radio.
    assert!(cable.hear(&dated(CABLE_BOARD, now)).is_none());
    assert!(cable.hear(&dated(CABLE_BOARD, now - 3_600_000)).is_none());
    let tip = cable.tip_time();
    cable.mine_to(2);
    assert!(cable.tip_time() <= tip + BLOCK_MS + CLOSE_MS, "{} {tip}", cable.tip_time());
}

/// A board that found no block for an hour and then restarts goes on from the date it saved, not
/// from its tip an hour back. A saved date never takes the clock below the tip.
#[test]
fn a_board_that_restarts_starts_from_the_date_it_saved_never_before_its_tip() {
    let coin = coin(0);
    let folder = fresh_folder("board_clock_restart");
    let now = unix_ms();
    let mut board = Board::start(&folder, &coin, CABLE_BOARD, None);
    board.hear(&dated(CABLE_BOARD, now));
    board.mine_to(2);
    let tip = board.tip_time();
    // An hour with no block of its own: mining off, or lent to the pool.
    let hour = 3_600_000;
    for _ in 0..hour / BLOCK_MS {
        board.uptime_ms += BLOCK_MS;
        board.engine.tick(board.uptime_ms);
        if board.keeping.due(board.uptime_ms) {
            board.saved = Some(record(board.engine.clock_at(board.uptime_ms)));
        }
    }
    let true_now = now + board.uptime_ms - BOOT_MS;
    let saved = board.saved;
    drop(board);
    let again = Board::start(&folder, &coin, CABLE_BOARD, saved);
    let clock = again.engine.clock_at(BOOT_MS);
    assert!(true_now.saturating_sub(clock) <= SAVE_EVERY_MS, "{} s behind", (true_now - clock) / 1_000);
    drop(again);
    let old = Board::start(&folder, &coin, CABLE_BOARD, Some(record(tip - hour)));
    assert!(old.engine.clock_at(BOOT_MS) >= tip);
}
