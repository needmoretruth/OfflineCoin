//! The numbers a node process is held to. How they are held is the system's own.
use super::{NodeSpec, PROCESS_OVERHEAD_MIB};
pub use olc_platform::Enforcement;
use olc_platform::Limits;
use std::sync::atomic::{AtomicU32, Ordering};

const MIB: u64 = 1024 * 1024;

/// The whole process's memory: what it was given for chain data, and what running takes.
pub fn memory_limit_bytes(spec: &NodeSpec) -> u64 {
    (spec.memory_mib + PROCESS_OVERHEAD_MIB) * MIB
}

/// The limits of the node `spec`, under a name no other node on this computer has.
pub fn limits_of(spec: &NodeSpec) -> Limits {
    // The system refuses a second group of one name, and the old one outlives its process for a
    // moment, so a node started again needs a name of its own.
    static STARTS: AtomicU32 = AtomicU32::new(0);
    let start = STARTS.fetch_add(1, Ordering::Relaxed);
    Limits {
        name: format!("{}-{}-{start}", spec.id, std::process::id()),
        memory_bytes: memory_limit_bytes(spec),
        cpu_percent: spec.cpu_percent,
    }
}

#[cfg(test)]
mod tests {
    use super::super::Preset;
    use super::*;

    #[test]
    fn a_node_started_again_gets_a_new_group_name() {
        let spec = Preset::Esp32S3.spec(3);
        assert_ne!(limits_of(&spec).name, limits_of(&spec).name);
    }

    #[test]
    fn an_esp32_node_gets_its_memory_and_no_cpu_share() {
        let limits = limits_of(&Preset::Esp32S3.spec(3));
        assert_eq!(limits.memory_bytes, (8 + PROCESS_OVERHEAD_MIB) * MIB);
        assert_eq!(limits.cpu_percent, 0);
    }

    #[test]
    fn a_cpu_share_is_kept() {
        assert_eq!(limits_of(&Preset::RaspberryPi4.spec(1)).cpu_percent, 400);
    }
}
