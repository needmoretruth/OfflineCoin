//! Puts the current firmware into the program, so `olc` alone can install and update boards.
//! Without a firmware build there is nothing to carry, and the program says so when asked.
use std::path::Path;
use std::{env, fs};

const ELF: &str = "../../firmware/target/xtensa-esp32s3-none-elf/release/olc-firmware";
const BUILD_ID: &str = "../../firmware/build-id";

fn main() {
    println!("cargo:rerun-if-changed={ELF}");
    println!("cargo:rerun-if-changed={BUILD_ID}");
    let out = env::var("OUT_DIR").expect("cargo sets OUT_DIR");
    let carried = Path::new(&out).join("firmware.elf");
    if fs::copy(ELF, &carried).is_err() {
        fs::write(&carried, []).expect("the build folder is writable");
    }
    let build = fs::read_to_string(BUILD_ID).unwrap_or_default();
    println!("cargo:rustc-env=OLC_CARRIED_BUILD={}", build.trim());
}
