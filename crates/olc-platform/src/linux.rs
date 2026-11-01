//! Linux: control groups through systemd, and `/proc` and `/sys` for everything else.
use crate::{unix, Enforcement, Limits, Platform, Usage, UsbPort};
use olc_node::network::Mark;
use olc_node::Link;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

/// Ticks per second of the kernel's CPU counters.
const TICKS: u64 = 100;
const PORT_ROOT: &str = "/sys/class/tty";

pub struct Os;

fn works(program: &str, words: &[&str]) -> bool {
    Command::new(program)
        .args(words)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn desktop_setting(key: &str) -> String {
    let words = ["get", "org.gnome.desktop.interface", key];
    let output = Command::new("gsettings").args(words).output();
    String::from_utf8_lossy(&output.map(|out| out.stdout).unwrap_or_default()).to_string()
}

fn word_of(folder: &Path, name: &str) -> String {
    let text = fs::read_to_string(folder.join(name)).unwrap_or_default();
    text.trim().to_string()
}

/// The USB device behind a tty: the interface's parent, and for some drivers one further up.
fn usb_device_of(tty: &Path) -> Option<PathBuf> {
    let interface = fs::canonicalize(tty.join("device")).ok()?;
    interface.ancestors().skip(1).take(2).find(|up| up.join("idVendor").exists()).map(Path::to_path_buf)
}

fn rss_bytes(pid: u32) -> Option<u64> {
    let text = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let line = text.lines().find(|line| line.starts_with("VmRSS:"))?;
    let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kib * 1024)
}

fn cpu_ms(pid: u32) -> Option<u64> {
    let text = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The name in brackets may hold spaces, so count from its end.
    let rest = text.rsplit_once(')')?.1;
    let fields: Vec<&str> = rest.split_whitespace().collect();
    let user: u64 = fields.get(11)?.parse().ok()?;
    let system: u64 = fields.get(12)?.parse().ok()?;
    Some((user + system) * 1000 / TICKS)
}

fn group_bytes(pid: u32) -> Option<u64> {
    let text = fs::read_to_string(format!("/proc/{pid}/cgroup")).ok()?;
    let path = text.lines().find_map(|line| line.strip_prefix("0::"))?;
    // Only a scope is this process's alone. Anything above it is shared with the session.
    path.ends_with(".scope").then_some(())?;
    let file = Path::new("/sys/fs/cgroup").join(path.trim_start_matches('/'));
    let charged: u64 = fs::read_to_string(file.join("memory.current")).ok()?.trim().parse().ok()?;
    let stat = fs::read_to_string(file.join("memory.stat")).ok()?;
    Some(charged.saturating_sub(file_cache(&stat)))
}

/// Page cache is charged to a group too, but the kernel gives it back whenever it is needed, so
/// a node that read its chain would look full of memory it is not using.
fn file_cache(stat: &str) -> u64 {
    let line = stat.lines().find_map(|line| line.strip_prefix("file "));
    line.and_then(|bytes| bytes.trim().parse().ok()).unwrap_or(0)
}

fn in_a_scope(limits: &Limits, program: &str, words: &[String]) -> Command {
    let mut command = Command::new("systemd-run");
    command.args(["--user", "--scope", "-q", "--collect"]);
    command.arg(format!("--unit=olc-node-{}", limits.name));
    command.arg("-p").arg(format!("MemoryMax={}", limits.memory_bytes));
    command.args(["-p", "MemorySwapMax=0"]);
    if limits.cpu_percent > 0 {
        command.arg("-p").arg(format!("CPUQuota={}%", limits.cpu_percent));
    }
    command.arg(program).args(words);
    command
}

fn in_address_space(limits: &Limits, program: &str, words: &[String]) -> Command {
    let mut command = Command::new("prlimit");
    // Address space counts reserved stacks and mapped code as well, so it gets more room than
    // resident memory would.
    command.arg(format!("--as={}", limits.memory_bytes * 8));
    command.arg("--").arg(program).args(words);
    command
}

impl Platform for Os {
    fn has_bluetooth(&self) -> bool {
        cfg!(feature = "ble")
    }

    #[cfg(feature = "ble")]
    fn bluetooth_link(&self, me: u64) -> Result<Box<dyn Link>, String> {
        Ok(Box::new(olc_link_ble::BleLink::start(me)?))
    }

    fn esp_now_link(&self, card: &str, mark: Mark) -> Result<Box<dyn Link>, String> {
        Ok(Box::new(olc_link_espnow::EspNowCard::open(card, mark)?))
    }

    fn data_folder(&self) -> Option<PathBuf> {
        Some(unix::home()?.join(".olc"))
    }

    fn config_folder(&self) -> Option<PathBuf> {
        Some(unix::home()?.join(".config/olc"))
    }

    fn host_name(&self) -> String {
        let name = fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default();
        name.trim().to_string()
    }

    fn device_model(&self) -> String {
        fs::read_to_string("/sys/firmware/devicetree/base/model").unwrap_or_default()
    }

    fn prefers_dark(&self) -> bool {
        desktop_setting("color-scheme").contains("dark")
    }

    fn reduces_motion(&self) -> bool {
        desktop_setting("enable-animations").contains("false")
    }

    fn usb_serial_ports(&self, vendor: u16, product: u16) -> Vec<UsbPort> {
        let Ok(entries) = fs::read_dir(PORT_ROOT) else {
            return Vec::new();
        };
        let wanted = (format!("{vendor:04x}"), format!("{product:04x}"));
        entries
            .flatten()
            .filter_map(|tty| {
                let device = usb_device_of(&tty.path())?;
                let found = (word_of(&device, "idVendor"), word_of(&device, "idProduct"));
                (found == wanted).then(|| UsbPort {
                    path: format!("/dev/{}", tty.file_name().to_string_lossy()),
                    serial: word_of(&device, "serial"),
                })
            })
            .collect()
    }

    fn keep_lines_on_close(&self, settings: &mut serial2::Settings) {
        unix::keep_lines_on_close(settings);
    }

    fn tool_folders(&self) -> Vec<PathBuf> {
        unix::home().map(|home| home.join(".local/bin")).into_iter().collect()
    }

    fn process_exists(&self, pid: u32) -> bool {
        Path::new(&format!("/proc/{pid}")).exists()
    }

    fn write_private(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        unix::write_private(path, bytes)
    }

    fn is_private(&self, path: &Path) -> bool {
        unix::is_private(path)
    }

    fn enforcement(&self) -> Enforcement {
        static FOUND: OnceLock<Enforcement> = OnceLock::new();
        *FOUND.get_or_init(|| {
            if works("systemd-run", &["--user", "--scope", "-q", "true"]) {
                Enforcement::Cgroup
            } else if works("prlimit", &["--as=1000000000", "true"]) {
                Enforcement::AddressSpace
            } else {
                Enforcement::Nothing
            }
        })
    }

    fn limited(&self, how: Enforcement, limits: &Limits, program: &str, words: &[String]) -> Command {
        match how {
            Enforcement::Cgroup => in_a_scope(limits, program, words),
            Enforcement::AddressSpace => in_address_space(limits, program, words),
            _ => {
                let mut command = Command::new(program);
                command.args(words);
                command
            }
        }
    }

    fn usage(&self, pid: u32) -> Option<Usage> {
        Some(Usage {
            rss_bytes: rss_bytes(pid)?,
            group_bytes: group_bytes(pid),
            cpu_ms: cpu_ms(pid)?,
        })
    }

    fn open_folder(&self, path: &Path) -> std::io::Result<()> {
        Command::new("xdg-open").arg(path).spawn().map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scope_holds_memory_and_cpu() {
        let limits = Limits { name: "1-2".into(), memory_bytes: 5000, cpu_percent: 40 };
        let command = in_a_scope(&limits, "olc-core", &["node".into()]);
        let words: Vec<_> = command.get_args().map(|word| word.to_string_lossy()).collect();
        assert!(words.iter().any(|word| word == "--unit=olc-node-1-2"));
        assert!(words.iter().any(|word| word == "MemoryMax=5000"));
        assert!(words.iter().any(|word| word == "CPUQuota=40%"));
    }

    #[test]
    fn a_node_without_a_share_gets_no_cpu_limit() {
        let limits = Limits { name: "1".into(), memory_bytes: 5000, cpu_percent: 0 };
        let command = in_a_scope(&limits, "olc-core", &[]);
        assert!(!command.get_args().any(|word| word.to_string_lossy().starts_with("CPUQuota")));
    }

    #[test]
    fn address_space_gets_room_beyond_resident_memory() {
        let limits = Limits { name: "1".into(), memory_bytes: 1000, cpu_percent: 0 };
        let command = in_address_space(&limits, "olc-core", &[]);
        assert!(command.get_args().any(|word| word == "--as=8000"));
    }
}
