//! `olc-fuzz [INPUTS] [SEED] [NAME]`: INPUTS per target. Ends with an error if anything panicked.
use olc_fuzz::{run_all, Outcome};
use std::process::ExitCode;

const DEFAULT_INPUTS: u64 = 1_000_000;
const DEFAULT_SEED: u64 = 1;

fn say(outcome: &Outcome) {
    let (name, inputs, panics) = (outcome.name, outcome.inputs, outcome.panics);
    let (taken, deep) = (outcome.taken, outcome.deep);
    let took = outcome.took.as_secs_f32();
    let slowest = outcome.slowest.as_secs_f32() * 1_000.0;
    println!(
        "{name:<22} {inputs:>9} inputs {taken:>9} taken {deep:>8} deep {panics:>7} panics \
         {took:>7.1} s  slowest {slowest:.1} ms"
    );
    for (place, found) in &outcome.found {
        let start: String = found.input.chars().take(160).collect();
        println!("  {} at {place}: {}\n    first input {start}", found.count, found.message);
    }
}

fn main() -> ExitCode {
    let mut words = std::env::args().skip(1);
    let count = words.next().and_then(|word| word.parse().ok()).unwrap_or(DEFAULT_INPUTS);
    let seed = words.next().and_then(|word| word.parse().ok()).unwrap_or(DEFAULT_SEED);
    let only = words.next();
    let outcomes = run_all(count, seed, only.as_deref(), say);
    let panics: u64 = outcomes.iter().map(|outcome| outcome.panics).sum();
    let places: usize = outcomes.iter().map(|outcome| outcome.found.len()).sum();
    let inputs: u64 = outcomes.iter().map(|outcome| outcome.inputs).sum();
    println!("{} targets, {inputs} inputs, {panics} panics at {places} places", outcomes.len());
    if panics == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
