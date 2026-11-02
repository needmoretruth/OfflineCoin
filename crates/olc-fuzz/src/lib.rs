//! Throws random and damaged bytes at every parser that bytes from outside can reach. A panic in
//! one of them is a crash any radio could cause, so each is counted, named and kept by place.
//!
//! Every run with one seed feeds the same inputs, so a failure can be run again.
mod chains;
mod control;
mod damage;
pub mod fixture;
mod local;
mod wire;

use std::collections::BTreeMap;
use std::cell::{Cell, RefCell};
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

/// What one target runs. It keeps whatever state the parser builds up between inputs.
pub type Parse = Box<dyn FnMut(&[u8])>;

pub struct Target {
    pub name: &'static str,
    /// Good inputs, to damage.
    pub seeds: fn() -> Vec<Vec<u8>>,
    /// Builds the parser. Called again after a panic, which may have left it broken.
    pub start: fn() -> Parse,
}

/// All panics of one target at one place in the source.
#[derive(Clone, Debug)]
pub struct Found {
    pub count: u64,
    pub message: String,
    /// The first input that caused it, in hex.
    pub input: String,
}

#[derive(Clone, Debug)]
pub struct Outcome {
    pub name: &'static str,
    pub inputs: u64,
    /// Inputs the first parser took, so a target that refuses everything shows for what it is.
    pub taken: u64,
    /// Inputs that changed or passed the deepest check of this target, where it has one.
    pub deep: u64,
    pub panics: u64,
    pub found: BTreeMap<String, Found>,
    pub slowest: Duration,
    pub took: Duration,
}

thread_local! {
    static LAST_PANIC: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
    static IN_PARSER: Cell<bool> = const { Cell::new(false) };
    static TAKEN: Cell<u64> = const { Cell::new(0) };
    static DEEP: Cell<u64> = const { Cell::new(0) };
}

/// A target calls this when its first parser takes an input.
pub(crate) fn taken() {
    TAKEN.set(TAKEN.get() + 1);
}

/// A target calls this when an input got through its deepest check.
pub(crate) fn deep() {
    DEEP.set(DEEP.get() + 1);
}

fn listen_for_panics() {
    panic::set_hook(Box::new(|info| {
        let place = info.location().map(ToString::to_string).unwrap_or_default();
        let message = info.payload_as_str().unwrap_or("no message").to_string();
        // Outside a parser, this is a mistake in this program, and it should say so.
        if !IN_PARSER.get() {
            eprintln!("{place}: {message}");
        }
        LAST_PANIC.with(|last| *last.borrow_mut() = Some((place, message)));
    }));
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn note_panic(outcome: &mut Outcome, input: &[u8]) {
    outcome.panics += 1;
    let (place, message) = LAST_PANIC.with(|last| last.borrow_mut().take()).unwrap_or_default();
    let first = || Found { count: 0, message, input: hex(input) };
    outcome.found.entry(place).or_insert_with(first).count += 1;
}

/// Feeds `count` inputs to one target.
pub fn run(target: &Target, count: u64, seed: u64) -> Outcome {
    let started = Instant::now();
    let mut outcome = Outcome {
        name: target.name,
        inputs: count,
        taken: 0,
        deep: 0,
        panics: 0,
        found: BTreeMap::new(),
        slowest: Duration::ZERO,
        took: Duration::ZERO,
    };
    let seeds = (target.seeds)();
    let mut rng = fastrand::Rng::with_seed(seed);
    let mut parse = (target.start)();
    let mut input = Vec::new();
    TAKEN.set(0);
    DEEP.set(0);
    for _ in 0..count {
        damage::next_input(&mut rng, &seeds, &mut input);
        let before = Instant::now();
        IN_PARSER.set(true);
        let result = panic::catch_unwind(AssertUnwindSafe(|| parse(&input)));
        IN_PARSER.set(false);
        outcome.slowest = outcome.slowest.max(before.elapsed());
        if result.is_err() {
            note_panic(&mut outcome, &input);
            parse = (target.start)();
        }
    }
    outcome.taken = TAKEN.get();
    outcome.deep = DEEP.get();
    outcome.took = started.elapsed();
    outcome
}

/// Every target there is.
pub fn targets() -> Vec<Target> {
    let mut all = wire::targets();
    all.extend(control::targets());
    all.extend(chains::targets());
    all.extend(local::targets());
    all
}

/// Runs the targets side by side, `count` inputs each. `only` keeps those whose name holds it.
/// `said` hears each outcome as it ends.
pub fn run_all(
    count: u64,
    seed: u64,
    only: Option<&str>,
    said: impl Fn(&Outcome) + Sync,
) -> Vec<Outcome> {
    listen_for_panics();
    let all = targets();
    let chosen: Vec<&Target> =
        all.iter().filter(|target| only.is_none_or(|word| target.name.contains(word))).collect();
    let next = AtomicUsize::new(0);
    let outcomes = Mutex::new(Vec::new());
    let workers = thread::available_parallelism().map_or(1, usize::from);
    thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                while let Some(target) = chosen.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let outcome = run(target, count, seed);
                    said(&outcome);
                    if let Ok(mut outcomes) = outcomes.lock() {
                        outcomes.push(outcome);
                    }
                }
            });
        }
    });
    let mut outcomes = outcomes.into_inner().unwrap_or_default();
    outcomes.sort_by_key(|outcome| outcome.name);
    outcomes
}
