//! What Linux and macOS do alike.
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

const OWNER_ONLY: u32 = 0o600;

pub fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(OWNER_ONLY)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

pub fn is_private(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|meta| meta.permissions().mode() & 0o777 == OWNER_ONLY)
}

/// Without this, closing the port drops its lines, and a board takes that as a reset.
pub fn keep_lines_on_close(settings: &mut serial2::Settings) {
    settings.as_termios_mut().c_cflag &= !libc::HUPCL;
}
