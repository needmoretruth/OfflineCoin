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

const BATCH: u32 = 5_000;
/// A block's time moves on and the waiting payments are taken in again this often.
const BUILD_EVERY: Duration = Duration::from_secs(5);
const WAIT: Duration = Duration::from_millis(1);
const IDLE: Duration = Duration::from_millis(5);

#[derive(Clone, Copy)]
struct Job {
    round: u64,
    header: BlockHeader,
}

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
    found: Mutex<Option<Job>>,
    counted: AtomicU64,
    cap: AtomicU64,
    /// The start of this second and the hashes granted in it.
    window: Mutex<(Instant, u64)>,
}

impl Shared {
    /// Hashes a thread may try now. The cap is for all threads together.
    fn grant(&self) -> u32 {
        let cap = self.cap.load(Ordering::Relaxed);
        if cap == 0 {
            return BATCH;
        }
        // A batch bigger than the cap allows in a window would overrun it.
        let batch = u32::try_from(cap / 100).map_or(BATCH, |share| share.clamp(1, BATCH));
        let Ok(mut window) = self.window.lock() else {
            return 0;
        };
        let mut elapsed = window.0.elapsed().as_millis() as u64;
        if elapsed >= 1_000 {
            *window = (Instant::now(), 0);
            elapsed = 0;
        }
        if window.1 >= cap.saturating_mul(elapsed) / 1_000 {
            return 0;
        }
        window.1 += u64::from(batch);
        batch
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
}

impl Default for Crew {
    fn default() -> Self {
        Self {
            shared: Arc::new(Shared {
                job: Mutex::new(None),
                round: AtomicU64::new(0),
                found: Mutex::new(None),
                counted: AtomicU64::new(0),
                cap: AtomicU64::new(0),
                window: Mutex::new((Instant::now(), 0)),
            }),
            hands: Vec::new(),
            block: None,
            basis: None,
            built: Instant::now(),
        }
    }
}

impl Crew {
    pub(super) fn size(&self) -> usize {
        self.hands.len()
    }

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

    pub(super) fn set_cap(&self, cap: u64) {
        self.shared.cap.store(cap, Ordering::Relaxed);
    }

    /// Hashes tried since the last call.
    pub(super) fn take_counted(&self) -> u64 {
        self.shared.counted.swap(0, Ordering::Relaxed)
    }

    /// Adds a block the threads sealed, and keeps them on the newest block to seal.
    pub(super) fn lead(&mut self, engine: &mut Engine, now_ms: u64) {
        let found = self.shared.found.lock().ok().and_then(|mut found| found.take());
        let round = self.shared.round.load(Ordering::Relaxed);
        if let Some(job) = found.filter(|job| job.round == round) {
            if let Some(mut block) = self.block.take() {
                block.header = job.header;
                self.basis = None;
                engine.submit_block(block);
            }
        }
        let basis = Basis::of(engine);
        let time_ms = now_ms.max(engine.chain().tip_header().time_ms + 1);
        if self.basis.as_ref() == Some(&basis) {
            // Only the time is out of date: building again would copy the unspent set.
            if let Some(block) = self.block.as_mut().filter(|_| self.built.elapsed() >= BUILD_EVERY) {
                block.header.time_ms = time_ms.max(block.header.time_ms);
                let header = block.header;
                self.publish(Some(header));
                self.built = Instant::now();
            }
            if self.block.is_some() {
                return;
            }
        }
        let block = engine.work_template(&basis.miner, time_ms, &basis.note);
        self.publish(Some(block.header));
        self.block = Some(block);
        self.basis = Some(basis);
        self.built = Instant::now();
    }

    /// Stops the threads from trying, as when mining is off or the node catches up.
    pub(super) fn rest(&mut self) {
        if self.block.take().is_some() {
            self.basis = None;
            self.publish(None);
        }
    }

    fn publish(&self, header: Option<BlockHeader>) {
        if let Ok(mut job) = self.shared.job.lock() {
            let round = self.shared.round.load(Ordering::Relaxed) + 1;
            *job = header.map(|header| Job { round, header });
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
        let difficulty = job.header.difficulty;
        let sealed = ProofOfWork.try_meet(&mut job.header, difficulty, granted);
        shared.counted.fetch_add(u64::from(granted), Ordering::Relaxed);
        if sealed {
            sealed_round = Some(job.round);
            if let Ok(mut found) = shared.found.lock() {
                *found = Some(*job);
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
        crew.publish(Some(header(u64::MAX)));
        thread::sleep(Duration::from_millis(100));
        crew.take_counted();
        thread::sleep(run);
        let counted = crew.take_counted();
        crew.rest();
        counted * 1_000 / run.as_millis() as u64
    }

    #[test]
    fn the_cap_holds_for_all_threads_together() {
        let rate = grind_for(4, 40_000, Duration::from_millis(2_000));
        assert!(rate <= 44_000, "{rate} hashes a second under a cap of 40,000");
        assert!(rate >= 30_000, "{rate} hashes a second under a cap of 40,000");
    }

    #[test]
    fn a_sealed_header_comes_back_from_a_thread() {
        let mut crew = Crew::default();
        crew.resize(3);
        crew.publish(Some(header(2)));
        let started = Instant::now();
        let job = loop {
            if let Some(job) = crew.shared.found.lock().unwrap().take() {
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

    /// Full speed on this computer against one core, as the brief measures it. It takes the whole
    /// machine for two seconds, so it runs only when asked: `cargo test -- --ignored full_speed`.
    #[test]
    #[ignore = "measures this computer; run alone"]
    fn full_speed_uses_the_threads() {
        let threads = standard_threads();
        let one = grind_for(1, 0, Duration::from_millis(2_000));
        let all = grind_for(threads, 0, Duration::from_millis(2_000));
        println!("one thread {one} H/s, {threads} threads {all} H/s, {:.2} of one core x threads",
            all as f64 / (one as f64 * threads as f64));
        assert!(all as f64 >= 0.8 * one as f64 * threads as f64);
    }
}
