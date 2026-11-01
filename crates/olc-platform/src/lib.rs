//! What differs between operating systems, and nothing else. Each system has one file here, and
//! a new system means one new file and one line below. Everything above this crate calls
//! [`platform`] and never asks which system it runs on.
#[cfg(not(target_os = "linux"))]
mod foreign;
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
mod os;
#[cfg(unix)]
mod unix;

use std::any::Any;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};

/// A USB serial device, as the system names it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UsbPort {
    pub path: String,
    /// The serial number the device gives. Empty when the system does not tell.
    pub serial: String,
}

/// What a node process may use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Limits {
    /// A name for the group holding it, unique on this computer.
    pub name: String,
    pub memory_bytes: u64,
    /// Share of one core. Zero means no limit.
    pub cpu_percent: u32,
}

/// What really holds a node to its numbers here. The screen says which one it got.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Enforcement {
    /// The kernel's control groups hold memory and CPU.
    Cgroup,
    /// Address space is held, and CPU is not.
    AddressSpace,
    /// A Windows job holds working memory and lowers priority. CPU share is not held.
    JobObject,
    /// Only a low priority. Memory is not held.
    Priority,
    Nothing,
}

/// What a process is using now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    pub rss_bytes: u64,
    /// What its own control group is charged, when it has one.
    pub group_bytes: Option<u64>,
    /// Processor time since it started.
    pub cpu_ms: u64,
}

/// Keeps a limit alive for as long as its process runs.
pub type Hold = Box<dyn Any + Send>;

pub trait Platform: Sync {
    /// Where nodes, coins and settings live.
    fn data_folder(&self) -> Option<PathBuf>;
    /// Where the control key and the list of proven builds live.
    fn config_folder(&self) -> Option<PathBuf>;
    fn host_name(&self) -> String;
    /// The computer's model, when it says. A Raspberry Pi is told apart by this.
    fn device_model(&self) -> String;

    /// Whether the desktop asks for dark colours, for when the window toolkit cannot tell.
    fn prefers_dark(&self) -> bool {
        false
    }
    fn reduces_motion(&self) -> bool {
        false
    }

    /// The serial ports of USB devices with these ids.
    fn usb_serial_ports(&self, vendor: u16, product: u16) -> Vec<UsbPort>;
    /// Keeps closing a port from toggling its lines, which would restart a board.
    fn keep_lines_on_close(&self, _settings: &mut serial2::Settings) {}
    /// Keeps a helper program from opening a window of its own.
    fn hide_window(&self, _command: &mut Command) {}
    /// Folders beyond the app's own and the search path where a helper program may be.
    fn tool_folders(&self) -> Vec<PathBuf> {
        Vec::new()
    }

    fn process_exists(&self, pid: u32) -> bool;
    fn write_private(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()>;
    fn is_private(&self, path: &Path) -> bool;

    fn enforcement(&self) -> Enforcement;
    /// The command that starts `program` under `limits`, by `how`.
    fn limited(&self, how: Enforcement, limits: &Limits, program: &str, words: &[String]) -> Command;
    /// Puts a started process under `limits`, where the system does that afterwards.
    fn hold(&self, _how: Enforcement, _limits: &Limits, _child: &Child) -> Option<Hold> {
        None
    }
    fn usage(&self, pid: u32) -> Option<Usage>;

    /// Shows a folder in the system's file manager.
    /// Shows a folder in the file manager. Web addresses go the same way.
    fn open_folder(&self, path: &Path) -> std::io::Result<()>;
}

/// This computer's system.
pub fn platform() -> &'static dyn Platform {
    &os::Os
}

/// A helper program: beside the app, in [`Platform::tool_folders`], or on the search path.
pub fn tool(name: &str) -> PathBuf {
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    let beside = std::env::current_exe().ok().and_then(|app| app.parent().map(Path::to_path_buf));
    let folders = beside.into_iter().chain(platform().tool_folders());
    folders
        .map(|folder| folder.join(&file))
        .find(|path| path.exists())
        .unwrap_or_else(|| PathBuf::from(file))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_folders_are_known() {
        assert!(platform().data_folder().is_some());
        assert!(platform().config_folder().is_some());
    }

    #[test]
    fn this_process_is_seen() {
        let pid = std::process::id();
        assert!(platform().process_exists(pid));
        assert!(!platform().process_exists(u32::MAX - 1));
        let usage = platform().usage(pid).expect("this process");
        assert!(usage.rss_bytes > 1024 * 1024);
    }

    #[test]
    fn a_private_file_reads_as_private() {
        let folder = std::env::temp_dir().join(format!("olc-platform-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("key");
        platform().write_private(&file, b"secret").unwrap();
        assert!(platform().is_private(&file));
        assert_eq!(std::fs::read(&file).unwrap(), b"secret");
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn a_limited_command_runs_the_program() {
        let limits = Limits { name: "test".into(), memory_bytes: 1 << 30, cpu_percent: 50 };
        let how = platform().enforcement();
        let command = platform().limited(how, &limits, "olc-core", &["node".into()]);
        let mut words: Vec<_> = command.get_args().map(|word| word.to_string_lossy()).collect();
        words.insert(0, command.get_program().to_string_lossy());
        assert!(words.iter().any(|word| word == "olc-core"));
        assert_eq!(words.last().map(AsRef::as_ref), Some("node"));
    }
}
