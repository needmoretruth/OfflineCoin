//! Nothing simulated ships. The test stand-in for boards stays under `cfg(test)`, and this reads
//! the built program to prove it, so a change that lets it through fails here.
use std::fs;

const MARKERS: [&str; 7] = ["SimBoard", "SimFleet", "Bench genesis", "desk-a", "garage", "--sim", "simulated"];

#[test]
fn the_program_holds_nothing_simulated() {
    let bytes = fs::read(env!("CARGO_BIN_EXE_olc-core")).expect("the built program");
    for marker in MARKERS {
        let found = bytes
            .windows(marker.len())
            .any(|window| window == marker.as_bytes());
        assert!(!found, "the program contains {marker}");
    }
}
