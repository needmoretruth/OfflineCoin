//! Measuring a process where there is no `/proc` to read.
use crate::Usage;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

pub fn usage(pid: u32) -> Option<Usage> {
    let mut system = System::new().ok()?;
    let pid = Pid::from_u32(pid);
    let wanted = ProcessRefreshKind::nothing().with_memory().with_cpu();
    system.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, wanted);
    let process = system.process(pid)?;
    Some(Usage {
        rss_bytes: process.memory(),
        group_bytes: None,
        cpu_ms: process.accumulated_cpu_time(),
    })
}
