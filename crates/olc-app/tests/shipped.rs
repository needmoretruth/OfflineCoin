//! Nothing simulated ships. The test stand-in for boards stays under `cfg(test)`, and this reads
//! the built program to prove it, so a change that lets it through fails here.
use std::fs;

const MARKERS: [&str; 7] = ["SimBoard", "SimFleet", "Bench genesis", "desk-a", "garage", "--sim", "simulated"];

#[test]
fn the_program_holds_nothing_simulated() {
    let bytes = fs::read(env!("CARGO_BIN_EXE_olc-core")).expect("the built program");
    for marker in MARKERS {
        // "garage" is also a word of the BIP 39 English list, which ships as one word per line.
        let listed = format!("\n{marker}\n");
        let found = bytes
            .windows(marker.len() + 2)
            .any(|window| window[1..=marker.len()] == *marker.as_bytes() && window != listed.as_bytes());
        assert!(!found, "the program contains {marker}");
    }
}
