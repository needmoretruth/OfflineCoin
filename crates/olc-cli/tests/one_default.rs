//! A board's default cap has one home. A second copy of the number would drift from it.
use std::fs;
use std::path::Path;

fn source(path: &str) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    fs::read_to_string(root.join(path)).expect("the file is in the repository")
}

#[test]
fn the_board_page_is_told_the_default_cap() {
    let script = source("crates/olc-cli/src/boards.js");
    assert!(script.contains("board.default_cap"));
    assert!(!script.contains("BOARD_RELAXED"));
    assert!(source("crates/olc-cli/src/fleet_api.rs")
        .contains("\"default_cap\": olc_control::defaults::BOARD_CAP"));
}

#[test]
fn the_firmware_reads_the_shared_defaults() {
    let node = source("firmware/src/node.rs");
    assert!(node.contains("defaults::BOARD_CAP as RELAXED_LIMIT"));
    assert!(node.contains("defaults::BOARD_SCREEN_RATE as DEFAULT_SMOOTH"));
    assert!(!node.contains("const RELAXED_LIMIT"));
    assert!(!node.contains("const DEFAULT_SMOOTH"));
    let clock = source("firmware/src/clock.rs");
    assert!(clock.contains("defaults::BOARD_CLOCK_MHZ as DEFAULT_MHZ"));
    assert!(!clock.contains("const DEFAULT_MHZ"));
}
