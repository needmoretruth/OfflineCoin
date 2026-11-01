//! Windows: the registry tells which COM port belongs to which USB device, and a job object
//! holds a node's working memory and priority. A job's CPU share is out of reach of safe code
//! here, so the screen says it is not held.
use crate::{foreign, Enforcement, Hold, Limits, Place, PlaceKind, Platform, Usage, UsbPort};
use std::fs::OpenOptions;
use std::io::Write;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use win32job::{ExtendedLimitInfo, Job, PriorityClass};
use winreg::enums::HKEY_LOCAL_MACHINE;
use winreg::RegKey;

const NO_WINDOW: u32 = 0x0800_0000;
const USB_KEY: &str = r"SYSTEM\CurrentControlSet\Enum\USB";
/// Windows keeps at least this much of a process in memory whatever the limit.
const SMALLEST_WORKING_SET: usize = 1024 * 1024;

pub struct Os;

fn app_folder() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|folder| PathBuf::from(folder).join("OLC"))
}

/// A composite device names its parts after a prefix its parent keeps, and only the parent
/// holds the serial number.
fn serial_of(usb: &RegKey, vendor: u16, product: u16, part: &str) -> String {
    let Ok(parent) = usb.open_subkey(format!("VID_{vendor:04X}&PID_{product:04X}")) else {
        return String::new();
    };
    let serials = parent.enum_keys().flatten();
    serials
        .filter_map(|serial| {
            let prefix: String = parent.open_subkey(&serial).ok()?.get_value("ParentIdPrefix").ok()?;
            part.starts_with(&prefix).then_some(serial)
        })
        .next()
        .unwrap_or_default()
}

fn job_for(limits: &Limits) -> Option<Job> {
    let memory = usize::try_from(limits.memory_bytes).ok()?;
    let mut info = ExtendedLimitInfo::new();
    info.limit_working_memory(SMALLEST_WORKING_SET.min(memory), memory)
        .limit_priority_class(PriorityClass::BelowNormal)
        .limit_kill_on_job_close();
    Job::create_with_limit_info(&info).ok()
}

impl Platform for Os {
    fn data_folder(&self) -> Option<PathBuf> {
        app_folder()
    }

    fn config_folder(&self) -> Option<PathBuf> {
        app_folder()
    }

    fn host_name(&self) -> String {
        std::env::var("COMPUTERNAME").unwrap_or_default()
    }

    fn device_model(&self) -> String {
        String::new()
    }

    fn usb_serial_ports(&self, vendor: u16, product: u16) -> Vec<UsbPort> {
        let live = serial2::SerialPort::available_ports().unwrap_or_default();
        let machine = RegKey::predef(HKEY_LOCAL_MACHINE);
        let Ok(usb) = machine.open_subkey(USB_KEY) else {
            return Vec::new();
        };
        let Ok(parts) = usb.open_subkey(format!("VID_{vendor:04X}&PID_{product:04X}&MI_00")) else {
            return Vec::new();
        };
        parts
            .enum_keys()
            .flatten()
            .filter_map(|part| {
                let key = parts.open_subkey(format!(r"{part}\Device Parameters")).ok()?;
                let path: String = key.get_value("PortName").ok()?;
                // The registry remembers ports of devices long gone.
                live.iter().any(|port| port.as_os_str() == path.as_str()).then(|| UsbPort {
                    serial: serial_of(&usb, vendor, product, &part),
                    path,
                })
            })
            .collect()
    }

    fn hide_window(&self, command: &mut Command) {
        command.creation_flags(NO_WINDOW);
    }

    fn process_exists(&self, pid: u32) -> bool {
        foreign::usage(pid).is_some()
    }

    /// The folder under the person's profile already shuts others out.
    fn write_private(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        file.write_all(bytes)?;
        file.sync_all()
    }

    fn is_private(&self, path: &Path) -> bool {
        path.exists()
    }

    fn enforcement(&self) -> Enforcement {
        Enforcement::JobObject
    }

    fn limited(&self, _how: Enforcement, _limits: &Limits, program: &str, words: &[String]) -> Command {
        let mut command = Command::new(program);
        command.args(words);
        command
    }

    fn hold(&self, _how: Enforcement, limits: &Limits, child: &Child) -> Option<Hold> {
        let job = job_for(limits)?;
        job.assign_process(child.as_raw_handle() as isize).ok()?;
        Some(Box::new(job))
    }

    fn usage(&self, pid: u32) -> Option<Usage> {
        foreign::usage(pid)
    }

    fn places(&self) -> Vec<Place> {
        let Some(home) = std::env::var_os("USERPROFILE").map(PathBuf::from) else {
            return Vec::new();
        };
        let mut places = crate::usual_places(
            &home,
            [
                (PlaceKind::Desktop, home.join("Desktop")),
                (PlaceKind::Documents, home.join("Documents")),
                (PlaceKind::Downloads, home.join("Downloads")),
            ],
        );
        for letter in 'D'..='Z' {
            let path = PathBuf::from(format!("{letter}:\\"));
            if path.is_dir() {
                let label = format!("{letter}:");
                places.push(Place { kind: PlaceKind::Drive, label, path });
            }
        }
        places
    }

    fn open_folder(&self, path: &Path) -> std::io::Result<()> {
        Command::new("explorer").arg(path).spawn().map(drop)
    }
}
