//! A firmware file a person names is the one that goes to boards, not the build carried inside.
use std::path::Path;
use std::process::Command;

#[test]
fn an_update_reads_the_firmware_file_it_was_given() {
    let folder = Path::new(env!("CARGO_TARGET_TMPDIR")).join("firmware-file");
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    let missing = folder.join("not-here.elf");
    let shown = Command::new(env!("CARGO_BIN_EXE_olc"))
        .args(["update", "ac276eaaef90", "--firmware"])
        .arg(&missing)
        .arg("--root")
        .arg(folder.join("app"))
        .arg("--data")
        .arg(folder.join("node"))
        .env("OLC_NO_USB", "1")
        .env("OLC_CONFIG", folder.join("config"))
        .output()
        .unwrap();
    let said = format!("{}{}", String::from_utf8_lossy(&shown.stdout), String::from_utf8_lossy(&shown.stderr));
    assert!(!shown.status.success(), "{said}");
    assert!(said.contains(&missing.display().to_string()), "the named file was not used: {said}");
}
