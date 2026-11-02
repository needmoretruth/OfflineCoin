//! A short run on every build. The long one is `olc-fuzz` on a release build.
use olc_fuzz::run_all;

const INPUTS: u64 = 3_000;
const SEED: u64 = 1;

#[test]
fn no_bytes_from_outside_make_a_parser_panic() {
    let outcomes = run_all(INPUTS, SEED, None, |_| {});
    assert!(outcomes.len() >= 30, "only {} targets", outcomes.len());
    let failed: Vec<_> = outcomes.iter().filter(|outcome| outcome.panics > 0).collect();
    assert!(failed.is_empty(), "{failed:#?}");
}
