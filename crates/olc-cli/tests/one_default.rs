//! A board's default cap has one home. A second copy of the number would drift from it.
use std::fs;
use std::path::Path;

fn source(path: &str) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    fs::read_to_string(root.join(path)).expect("the file is in the repository")
}

#[test]
fn the_app_reads_the_shared_cap() {
    assert!(source("crates/olc-service/src/settings.rs")
        .contains("BOARD_CAP: u64 = olc_control::defaults::BOARD_CAP"));
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
    let screen = source("firmware/src/screen.rs");
    assert!(screen.contains("defaults::BOARD_BRIGHT as DEFAULT_BRIGHT"));
    assert!(!screen.contains("const DEFAULT_BRIGHT"));
    assert!(source("firmware/src/store.rs").contains("defaults::BOARD_BLUETOOTH"));
    assert!(!source("firmware/src/main.rs").contains("settings.bluetooth =="));
}
