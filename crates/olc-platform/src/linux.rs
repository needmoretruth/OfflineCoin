//! Linux: control groups through systemd, and `/proc` and `/sys` for everything else.
use crate::{unix, Enforcement, Limits, NoBluetooth, Place, PlaceKind, Platform, Usage, UsbPort};
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

/// A folder from `user-dirs.dirs`, which names them in the desktop's language.
fn user_folder(home: &Path, listing: &str, key: &str, fallback: &str) -> PathBuf {
    let line = listing.lines().find_map(|line| line.trim().strip_prefix(key));
    let value = line.and_then(|rest| rest.strip_prefix('=')).map(|value| value.trim_matches('"'));
    match value.and_then(|value| value.strip_prefix("$HOME/")) {
        Some(name) => home.join(name),
        None => home.join(fallback),
    }
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

fn threads(pid: u32) -> Option<usize> {
    let text = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let line = text.lines().find(|line| line.starts_with("Threads:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

/// An address as the kernel's socket table writes it: each 32-bit word in the machine's order.
fn table_address(hex: &str) -> Option<std::net::IpAddr> {
    let mut bytes = Vec::with_capacity(16);
    for at in (0..hex.len()).step_by(8) {
        let word = u32::from_str_radix(hex.get(at..at + 8)?, 16).ok()?;
        bytes.extend(u32::from_ne_bytes(word.to_be_bytes()).to_be_bytes());
    }
    match bytes.len() {
        4 => Some(std::net::Ipv4Addr::new(bytes[0], bytes[1], bytes[2], bytes[3]).into()),
        16 => <[u8; 16]>::try_from(bytes).ok().map(|octets| std::net::Ipv6Addr::from(octets).into()),
        _ => None,
    }
}

/// The local addresses in one of the kernel's TCP tables that listen on `port`.
fn listening_in(table: &str, port: u16) -> Vec<std::net::IpAddr> {
    const LISTEN: &str = "0A";
    let rows = table.lines().skip(1).map(|line| line.split_whitespace().collect::<Vec<_>>());
    rows.filter(|fields| fields.get(3) == Some(&LISTEN))
        .filter_map(|fields| fields.get(1)?.split_once(':').map(|(address, at)| (address.to_string(), at.to_string())))
        .filter(|(_, at)| u16::from_str_radix(at, 16).ok() == Some(port))
        .filter_map(|(address, _)| table_address(&address))
        .collect()
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
    /// GNOME and most desktops carry zenity, and KDE carries kdialog.
    fn show_error(&self, title: &str, message: &str) {
        let zenity = ["--error", "--no-markup", "--title", title, "--text", message];
        if Command::new("zenity").args(zenity).status().is_ok() {
            return;
        }
        let _ = Command::new("kdialog").args(["--title", title, "--error", message]).status();
    }

    fn no_bluetooth(&self) -> Option<NoBluetooth> {
        if !cfg!(feature = "ble") {
            return Some(NoBluetooth::Build);
        }
        let adapters = fs::read_dir("/sys/class/bluetooth").map_or(0, Iterator::count);
        (adapters == 0).then_some(NoBluetooth::Adapter)
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

    fn language(&self) -> Option<String> {
        crate::language::posix(|name| std::env::var(name).ok())
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
            threads: threads(pid),
        })
    }

    fn listening_on(&self, port: u16) -> Option<Vec<std::net::IpAddr>> {
        let four = fs::read_to_string("/proc/net/tcp").ok()?;
        let six = fs::read_to_string("/proc/net/tcp6").unwrap_or_default();
        let mut found = listening_in(&four, port);
        found.extend(listening_in(&six, port));
        Some(found)
    }

    fn places(&self) -> Vec<Place> {
        let Some(home) = unix::home() else {
            return Vec::new();
        };
        let listing = fs::read_to_string(home.join(".config/user-dirs.dirs")).unwrap_or_default();
        let folder = |key, fallback| user_folder(&home, &listing, key, fallback);
        let mut places = crate::usual_places(
            &home,
            [
                (PlaceKind::Desktop, folder("XDG_DESKTOP_DIR", "Desktop")),
                (PlaceKind::Documents, folder("XDG_DOCUMENTS_DIR", "Documents")),
                (PlaceKind::Downloads, folder("XDG_DOWNLOAD_DIR", "Downloads")),
            ],
        );
        let user = home.file_name().map(PathBuf::from).unwrap_or_default();
        for root in [Path::new("/run/media").join(&user), Path::new("/media").join(&user)] {
            places.extend(crate::drives_in(&root));
        }
        places
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

    #[test]
    fn the_kernel_tables_read_as_addresses() {
        let head = "  sl  local_address rem_address   st\n";
        let four = format!("{head}   0: 0100007F:0D05 00000000:0000 0A\n   1: 00000000:0D06 00000000:0000 0A\n   2: 0100007F:0D05 0100007F:9999 01\n");
        assert_eq!(listening_in(&four, 3333), ["127.0.0.1".parse::<std::net::IpAddr>().unwrap()]);
        assert_eq!(listening_in(&four, 3334), ["0.0.0.0".parse::<std::net::IpAddr>().unwrap()]);
        let six = format!("{head}   0: 00000000000000000000000001000000:0D05 00000000000000000000000000000000:0000 0A\n");
        assert_eq!(listening_in(&six, 3333), ["::1".parse::<std::net::IpAddr>().unwrap()]);
    }

    #[test]
    fn a_socket_this_test_opens_is_listed() {
        let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = socket.local_addr().unwrap().port();
        let found = Os.listening_on(port).unwrap();
        assert_eq!(found, ["127.0.0.1".parse::<std::net::IpAddr>().unwrap()]);
    }
}
