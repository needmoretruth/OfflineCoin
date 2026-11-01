//! What a process really uses, read from the operating system.
use std::fs;
use std::path::Path;
use std::time::Instant;

/// Ticks per second of the kernel's CPU counters on Linux.
const TICKS: f64 = 100.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Measured {
    pub rss_bytes: u64,
    /// What its control group is charged, when it has one of its own.
    pub cgroup_bytes: Option<u64>,
    pub cpu_percent: f64,
    pub disk_bytes: u64,
}

pub fn rss_bytes(pid: u32) -> Option<u64> {
    let text = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let line = text.lines().find(|line| line.starts_with("VmRSS:"))?;
    let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kib * 1024)
}

fn cpu_ticks(pid: u32) -> Option<u64> {
    let text = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The name in brackets may hold spaces, so count from its end.
    let rest = text.rsplit_once(')')?.1;
    let fields: Vec<&str> = rest.split_whitespace().collect();
    let user: u64 = fields.get(11)?.parse().ok()?;
    let system: u64 = fields.get(12)?.parse().ok()?;
    Some(user + system)
}

fn cgroup_bytes(pid: u32) -> Option<u64> {
    let text = fs::read_to_string(format!("/proc/{pid}/cgroup")).ok()?;
    let path = text.lines().find_map(|line| line.strip_prefix("0::"))?;
    // Only a scope is this process's alone. Anything above it is shared with the session.
    path.ends_with(".scope").then_some(())?;
    let file = Path::new("/sys/fs/cgroup").join(path.trim_start_matches('/'));
    fs::read_to_string(file.join("memory.current"))
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// Follows one process, since CPU use is a difference between two looks.
pub struct Meter {
    pid: u32,
    last: Option<(Instant, u64)>,
    percent: f64,
}

impl Meter {
    pub fn new(pid: u32) -> Self {
        Self {
            pid,
            last: None,
            percent: 0.0,
        }
    }

    pub fn read(&mut self, disk_bytes: u64) -> Measured {
        if let Some(ticks) = cpu_ticks(self.pid) {
            if let Some((when, before)) = self.last {
                let seconds = when.elapsed().as_secs_f64();
                if seconds >= 1.0 {
                    let used = ticks.saturating_sub(before) as f64 / TICKS;
                    self.percent = used * 100.0 / seconds;
                    self.last = Some((Instant::now(), ticks));
                }
            } else {
                self.last = Some((Instant::now(), ticks));
            }
        }
        Measured {
            rss_bytes: rss_bytes(self.pid).unwrap_or(0),
            cgroup_bytes: cgroup_bytes(self.pid),
            cpu_percent: self.percent,
            disk_bytes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_can_be_measured() {
        let pid = std::process::id();
        assert!(rss_bytes(pid).unwrap() > 1024 * 1024);
        assert!(cpu_ticks(pid).is_some());
        let mut meter = Meter::new(pid);
        assert!(meter.read(5).rss_bytes > 0);
    }
}
