//! The shape of the workspace, checked: the core knows no window, and only the platform crate's
//! per-system files know which system this is.
use std::fs;
use std::path::{Path, PathBuf};

/// These crates do the work, and a window is no part of it.
const NO_WINDOW_HERE: [&str; 8] = [
    "olc-board",
    "olc-link-serial",
    "olc-link-tcp",
    "olc-node",
    "olc-platform",
    "olc-service",
    "olc-store",
    "olc-wallet",
];
const WINDOW_CRATES: [&str; 4] = ["eframe", "egui", "wgpu", "olc-app"];

/// The only files that may name a system.
const PLATFORM_FILES: [&str; 6] =
    ["lib.rs", "linux.rs", "macos.rs", "windows.rs", "unix.rs", "foreign.rs"];

const SYSTEM_WORDS: [&str; 15] = [
    "\"/proc",
    "\"/sys/",
    "\"/dev/",
    "/dev/tty",
    "std::os::",
    "systemd-run",
    "xdg-open",
    "cfg(target_os",
    "cfg(unix",
    "cfg(windows",
    "cfg(not(unix",
    "cfg(not(windows",
    "\"HOME\"",
    "\"APPDATA\"",
    "gsettings",
];

/// The window's own build switch, which is no logic.
const ALLOWED: &str = "windows_subsystem";

fn crates_folder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

fn rust_files(folder: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(folder).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, found);
        } else if path.extension().is_some_and(|end| end == "rs") {
            found.push(path);
        }
    }
}

fn is_platform_file(path: &Path) -> bool {
    let in_platform = path.components().any(|part| part.as_os_str() == "olc-platform");
    let name = path.file_name().unwrap().to_string_lossy();
    in_platform && PLATFORM_FILES.contains(&name.as_ref())
}

fn system_words_in(text: &str) -> Vec<&'static str> {
    let lines = text.lines().filter(|line| !line.contains(ALLOWED));
    let lines: Vec<_> = lines.collect();
    let named = |word: &&str| lines.iter().any(|line| line.contains(word));
    SYSTEM_WORDS.into_iter().filter(|word| named(word)).collect()
}

#[test]
fn the_core_crates_know_no_window() {
    for name in NO_WINDOW_HERE {
        let manifest = crates_folder().join(name).join("Cargo.toml");
        let text = fs::read_to_string(&manifest).unwrap();
        for window in WINDOW_CRATES {
            let named = text.lines().any(|line| line.trim_start().starts_with(window));
            assert!(!named, "{name} depends on {window}");
        }
    }
}

#[test]
fn only_platform_files_name_a_system() {
    let mut files = Vec::new();
    for entry in fs::read_dir(crates_folder()).unwrap().flatten() {
        let source = entry.path().join("src");
        if source.is_dir() {
            rust_files(&source, &mut files);
        }
    }
    assert!(files.len() > 100, "the scan found too little: {}", files.len());
    let mut wrong = Vec::new();
    for file in files.iter().filter(|file| !is_platform_file(file)) {
        let words = system_words_in(&fs::read_to_string(file).unwrap());
        if !words.is_empty() {
            wrong.push(format!("{}: {words:?}", file.display()));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

#[test]
fn the_check_sees_what_it_is_for() {
    assert_eq!(system_words_in("let a = \"/proc/1/stat\";"), ["\"/proc"]);
    assert_eq!(system_words_in("#[cfg(windows)]"), ["cfg(windows"]);
    assert!(system_words_in("let a = 1;").is_empty());
}
