//! What a process really uses, read from the operating system.
use olc_platform::platform;
use std::time::Instant;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Measured {
    pub rss_bytes: u64,
    /// What its control group is charged, when it has one of its own.
    pub cgroup_bytes: Option<u64>,
    pub cpu_percent: f64,
    pub disk_bytes: u64,
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
        let usage = platform().usage(self.pid);
        if let Some(used) = usage {
            if let Some((when, before)) = self.last {
                let seconds = when.elapsed().as_secs_f64();
                if seconds >= 1.0 {
                    let spent = used.cpu_ms.saturating_sub(before) as f64 / 1000.0;
                    self.percent = spent * 100.0 / seconds;
                    self.last = Some((Instant::now(), used.cpu_ms));
                }
            } else {
                self.last = Some((Instant::now(), used.cpu_ms));
            }
        }
        Measured {
            rss_bytes: usage.map_or(0, |used| used.rss_bytes),
            cgroup_bytes: usage.and_then(|used| used.group_bytes),
            cpu_percent: self.percent,
            disk_bytes,
        }
    }
}

/// Whether the process is there at all.
pub fn is_running(pid: u32) -> bool {
    platform().process_exists(pid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_can_be_measured() {
        let mut meter = Meter::new(std::process::id());
        assert!(meter.read(5).rss_bytes > 0);
    }
}
