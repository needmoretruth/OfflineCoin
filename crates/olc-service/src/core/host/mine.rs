//! Mining on several cores, as cpuminer does with `--threads`. The node's thread builds the block
//! and adds it; the threads here only try nonces on its header, each in its own part of the range,
//! and share one hash cap between them.
use olc_core::{Address, Block, BlockHeader, Consensus, Hash};
use olc_node::Engine;
use olc_pow::ProofOfWork;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use super::speed::Windows;

const BATCH: u32 = 5_000;
/// A block's time moves on and the waiting payments are taken in again this often.
const BUILD_EVERY: Duration = Duration::from_secs(5);
const WAIT: Duration = Duration::from_millis(1);
const IDLE: Duration = Duration::from_millis(5);

#[derive(Clone, Copy)]
struct Job {
    round: u64,
    header: BlockHeader,
    /// What the header must reach: its own difficulty for a block, less for a pool's share.
    difficulty: u64,
    /// Work from outside wants every header that reaches the difficulty, not only the first.
    every: bool,
}

/// Shares kept until the node's thread takes them; past this a stuck reader loses the oldest.
const MOST_FOUND: usize = 256;

/// What a block was built from. Any change means building it again.
#[derive(PartialEq, Eq)]
struct Basis {
    tip: Hash,
    miner: Address,
    note: Vec<u8>,
    waiting: usize,
}

impl Basis {
    fn of(engine: &Engine) -> Self {
        Self {
            tip: engine.chain().tip(),
            miner: engine.miner(),
            note: engine.note().to_vec(),
            waiting: engine.pool().len(),
        }
    }
}

struct Shared {
    job: Mutex<Option<Job>>,
    round: AtomicU64,
    found: Mutex<Vec<Job>>,
    cap: AtomicU64,
    /// The cap's windows and the hashes granted in each, which the speed is read from.
    windows: Mutex<Windows>,
}

impl Shared {
    /// Hashes a thread may try now. The cap is for all threads together.
    fn grant(&self) -> u32 {
        let cap = self.cap.load(Ordering::Relaxed);
        // A batch bigger than the cap allows in a window would overrun it.
        let batch = if cap == 0 { BATCH } else { u32::try_from(cap / 100).map_or(BATCH, |share| share.clamp(1, BATCH)) };
        self.windows.lock().map_or(0, |mut windows| windows.grant(cap, batch, Instant::now()))
    }
}

struct Hand {
    stop: Arc<AtomicBool>,
    done: JoinHandle<()>,
}

pub(super) struct Crew {
    shared: Arc<Shared>,
    hands: Vec<Hand>,
    block: Option<Block>,
    basis: Option<Basis>,
    built: Instant,
    /// The header from outside being ground, and its difficulty.
    outside: Option<(BlockHeader, u64)>,
}

impl Default for Crew {
    fn default() -> Self {
        Self {
            shared: Arc::new(Shared {
                job: Mutex::new(None),
                round: AtomicU64::new(0),
                found: Mutex::new(Vec::new()),
                cap: AtomicU64::new(0),
                windows: Mutex::new(Windows::new(Instant::now())),
            }),
            hands: Vec::new(),
            block: None,
            basis: None,
            built: Instant::now(),
            outside: None,
        }
    }
}

impl Crew {
    pub(super) fn resize(&mut self, wanted: usize) {
        if wanted == self.hands.len() {
            return;
        }
        self.stop_all();
        for index in 0..wanted {
            let stop = Arc::new(AtomicBool::new(false));
            let (shared, own) = (self.shared.clone(), stop.clone());
            let start = (u32::MAX / wanted as u32).saturating_mul(index as u32);
            let done = thread::spawn(move || work(&shared, &own, start));
            self.hands.push(Hand { stop, done });
        }
    }

    /// The threads that hash now.
    pub(super) fn size(&self) -> usize {
        self.hands.len()
    }

    pub(super) fn set_cap(&self, cap: u64) {
        self.shared.cap.store(cap, Ordering::Relaxed);
    }

    /// Hashes a second over the last five seconds, never above the cap.
    pub(super) fn speed(&self) -> u64 {
        let cap = self.shared.cap.load(Ordering::Relaxed);
        self.shared.windows.lock().map_or(0, |mut windows| windows.speed(cap, Instant::now()))
    }

    /// Adds a block the threads sealed, and keeps them on the newest block to seal.
    pub(super) fn lead(&mut self, engine: &mut Engine, now_ms: u64) {
        let found = self.take_found();
        let round = self.shared.round.load(Ordering::Relaxed);
        if let Some(job) = found.into_iter().find(|job| job.round == round && !job.every) {
            if let Some(mut block) = self.block.take() {
                block.header = job.header;
                self.basis = None;
                engine.submit_block(block);
            }
        }
        let basis = Basis::of(engine);
        let time_ms = now_ms.max(engine.chain().tip_header().time_ms + 1);
        if self.basis.as_ref() == Some(&basis) {
            // Only the time is out of date: building again would copy the unspent set. The engine
            // gives the time as the coin carries it and the difficulty that time asks, at once
            // when a late block's floor comes.
            let due = |block: &Block| self.built.elapsed() >= BUILD_EVERY || engine.floor_came(&block.header, now_ms);
            if let Some(block) = self.block.as_mut().filter(|block| due(block)) {
                let header = engine.retimed(&block.header, now_ms);
                self.built = Instant::now();
                if header != block.header {
                    block.header = header;
                    self.publish(Some((header, header.difficulty, false)));
                }
            }
            if self.block.is_some() {
                return;
            }
        }
        let block = engine.work_template(&basis.miner, time_ms, &basis.note);
        self.publish(Some((block.header, block.header.difficulty, false)));
        self.block = Some(block);
        self.basis = Some(basis);
        self.built = Instant::now();
    }

    /// Stops the threads from trying, as when mining is off or the node catches up.
    pub(super) fn rest(&mut self) {
        if let Ok(mut windows) = self.shared.windows.lock() {
            windows.restart(Instant::now());
        }
        if self.block.take().is_some() || self.outside.take().is_some() {
            self.basis = None;
            self.publish(None);
        }
    }

    /// Grinds a header from outside, a pool's, to its share difficulty on every thread, in place
    /// of mining. Returns the headers that reached it since the last call.
    pub(super) fn follow(&mut self, header: BlockHeader, difficulty: u64) -> Vec<BlockHeader> {
        if self.outside != Some((header, difficulty)) {
            self.block = None;
            self.basis = None;
            self.outside = Some((header, difficulty));
            self.publish(Some((header, difficulty, true)));
        }
        // A share of the job before still goes: the pool says whether it came in time.
        let found = self.take_found().into_iter().filter(|job| job.every);
        found.map(|job| job.header).collect()
    }

    fn take_found(&self) -> Vec<Job> {
        self.shared.found.lock().map(|mut found| std::mem::take(&mut *found)).unwrap_or_default()
    }

    fn publish(&self, work: Option<(BlockHeader, u64, bool)>) {
        if let Ok(mut job) = self.shared.job.lock() {
            let round = self.shared.round.load(Ordering::Relaxed) + 1;
            *job = work.map(|(header, difficulty, every)| Job { round, header, difficulty, every });
            self.shared.round.store(round, Ordering::Release);
        }
    }

    fn stop_all(&mut self) {
        for hand in &self.hands {
            hand.stop.store(true, Ordering::Relaxed);
        }
        for hand in self.hands.drain(..) {
            let _ = hand.done.join();
        }
    }
}

impl Drop for Crew {
    fn drop(&mut self) {
        self.stop_all();
    }
}

/// One thread's turn: take the newest header, try nonces from `start` on, say when one seals it.
fn work(shared: &Shared, stop: &AtomicBool, start: u32) {
    let mut held: Option<Job> = None;
    let mut sealed_round = None;
    while !stop.load(Ordering::Relaxed) {
        let round = shared.round.load(Ordering::Acquire);
        if held.is_none_or(|job| job.round != round) {
            held = shared.job.lock().ok().and_then(|job| *job).map(|mut job| {
                job.header.nonce = start;
                job
            });
        }
        let Some(job) = held.as_mut().filter(|job| sealed_round != Some(job.round)) else {
            thread::sleep(IDLE);
            continue;
        };
        let granted = shared.grant();
        if granted == 0 {
            thread::sleep(WAIT);
            continue;
        }
        let sealed = ProofOfWork.try_meet(&mut job.header, job.difficulty, granted);
        if sealed {
            if let Ok(mut found) = shared.found.lock() {
                if found.len() >= MOST_FOUND {
                    found.remove(0);
                }
                found.push(*job);
            }
            match job.every {
                true => job.header.nonce = job.header.nonce.wrapping_add(1),
                false => sealed_round = Some(job.round),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::standard_threads;

    fn header(difficulty: u64) -> BlockHeader {
        BlockHeader {
            version: 1,
            previous: [0; 32],
            merkle_root: [0; 32],
            time_ms: 1,
            difficulty,
            nonce: 0,
        }
    }

    /// Several threads grind one header that cannot be sealed, under a cap, for `run`.
    fn grind_for(threads: usize, cap: u64, run: Duration) -> u64 {
        let mut crew = Crew::default();
        crew.set_cap(cap);
        crew.resize(threads);
        crew.publish(Some((header(u64::MAX), u64::MAX, false)));
        thread::sleep(run);
        let speed = crew.speed();
        crew.rest();
        speed
    }

    #[test]
    fn the_cap_holds_for_all_threads_together() {
        let rate = grind_for(4, 40_000, Duration::from_millis(2_000));
        assert!(rate <= 44_000, "{rate} hashes a second under a cap of 40,000");
        assert!(rate >= 30_000, "{rate} hashes a second under a cap of 40,000");
    }

    /// Held up for most of a second, the crew may make up a twentieth of a second at once, not the
    /// whole lag, so a one second reading that straddles two windows stays near the cap.
    #[test]
    fn a_crew_that_fell_behind_makes_up_only_a_little() {
        let start = Instant::now();
        let mut windows = Windows::new(start);
        windows.grant(10_000, 100, start);
        let mut granted = 0;
        loop {
            let batch = windows.grant(10_000, 100, start + Duration::from_millis(900));
            if batch == 0 {
                break;
            }
            granted += u64::from(batch);
        }
        assert!(granted <= 1_000, "{granted} hashes at once after a stall, against 9,000 owed");
    }

    #[test]
    fn a_sealed_header_comes_back_from_a_thread() {
        let mut crew = Crew::default();
        crew.resize(3);
        crew.publish(Some((header(2), 2, false)));
        let started = Instant::now();
        let job = loop {
            if let Some(job) = crew.shared.found.lock().unwrap().pop() {
                break job;
            }
            assert!(started.elapsed() < Duration::from_secs(5), "no thread sealed an easy header");
            thread::sleep(WAIT);
        };
        assert!(ProofOfWork.check_seal(&job.header));
    }

    #[test]
    fn a_computer_mines_on_all_but_one_core() {
        let cores = thread::available_parallelism().map_or(1, |cores| cores.get());
        assert_eq!(standard_threads(), cores.saturating_sub(1).max(1));
    }

    /// Full speed on this computer against one thread alone. Two threads of one core share its
    /// hashing, so the bar counts the cores the threads stand on. It takes the whole machine for
    /// a few seconds, so it runs only when asked: `cargo test -- --ignored full_speed`.
    #[test]
    #[ignore = "measures this computer; run alone"]
    fn full_speed_uses_the_threads() {
        let threads = standard_threads();
        let physical = olc_platform::physical_cores();
        let one = grind_for(1, 0, Duration::from_millis(2_000));
        let on_cores = grind_for(physical.saturating_sub(1).max(1), 0, Duration::from_millis(2_000));
        let all = grind_for(threads, 0, Duration::from_millis(2_000));
        let used = threads.min(physical);
        println!(
            "one thread {one} H/s; {} threads {on_cores} H/s; {threads} threads {all} H/s on {physical} cores; \
             {:.2} of one thread x {used} cores, {:.2} of one thread x {threads} threads",
            physical.saturating_sub(1).max(1),
            all as f64 / (one as f64 * used as f64),
            all as f64 / (one as f64 * threads as f64),
        );
        assert!(all as f64 >= 0.8 * one as f64 * used as f64);
        assert!(all >= on_cores, "fewer threads would mine more");
    }

    /// The highest speed the app shows for a thread cap is measured on that many threads, so it
    /// is not above what the miner really does on them: with one thread and with every core,
    /// where one thread's speed times the threads would promise what two threads of one core
    /// never do. It takes the whole machine, so it runs only when asked:
    /// `cargo test -- --ignored highest_speed`.
    #[test]
    #[ignore = "measures this computer; run alone"]
    fn the_highest_speed_is_not_above_a_real_run_on_those_threads() {
        let all = crate::settings::cores();
        let rates = crate::core::rates_by_threads(all);
        for threads in [1, all] {
            let shown = crate::core::rate_on(&rates, threads);
            let real = grind_for(threads, 0, Duration::from_millis(2_000));
            let times_one = rates[0] * threads as u64;
            println!("{threads} threads: shown {shown}, real run {real}, one thread times {threads} {times_one}");
            assert!(shown as f64 <= real as f64 * 1.10, "{shown} shown on {threads} threads is above the real run {real}");
        }
    }

    /// A thread cap reads the speed measured on that many threads, not one thread's times them,
    /// and a cap past the measured counts reads the last.
    #[test]
    fn the_highest_speed_is_read_for_the_threads_in_use() {
        let rates = [100, 190, 230, 260];
        assert_eq!(crate::core::rate_on(&rates, 1), 100);
        assert_eq!(crate::core::rate_on(&rates, 3), 230, "not 3 x 100");
        assert_eq!(crate::core::rate_on(&rates, 9), 260);
        assert_eq!(crate::core::rate_on(&rates, 0), 100);
        assert_eq!(crate::core::rate_on(&[], 4), 0);
    }

    /// A block's time moves on as the coin carries it, with the difficulty that time asks: whole
    /// seconds on Bitcoin's header, and the floor the moment a late block may take it.
    #[test]
    fn a_moved_time_keeps_the_rules() {
        let mut coin = olc_core::CoinConfig::olc();
        coin.initial_difficulty = 1 << 40;
        let plugins = olc_core::Plugins { consensus: Box::new(ProofOfWork), languages: Vec::new() };
        let wallet = olc_wallet::Wallet::from_secret([3; 32]).unwrap();
        let mut engine = Engine::new(coin.clone(), plugins, wallet);
        let published = |crew: &Crew| crew.shared.job.lock().unwrap().unwrap().header;
        let mut crew = Crew::default();
        let start = coin.genesis_time_ms + 1_234;
        crew.lead(&mut engine, start);
        let first = published(&crew);
        assert_eq!(first.time_ms, coin.genesis_time_ms + 1_000);
        crew.built -= BUILD_EVERY;
        crew.lead(&mut engine, start + 7_500);
        let moved = published(&crew);
        assert_eq!((moved.time_ms, moved.difficulty), (coin.genesis_time_ms + 8_000, first.difficulty));
        let late = engine.chain().floor_after(&engine.chain().tip()).unwrap();
        crew.lead(&mut engine, late + 1_400);
        let floored = published(&crew);
        assert_eq!((floored.time_ms, floored.difficulty), (late + 1_000, coin.rules.min_difficulty));
    }
}
