//! The numbers a node process is held to. How they are held is the system's own.
use super::{NodeSpec, PROCESS_OVERHEAD_MIB};
pub use olc_platform::Enforcement;
use olc_platform::Limits;

const MIB: u64 = 1024 * 1024;

/// The whole process's memory: what it was given for chain data, and what running takes.
pub fn memory_limit_bytes(spec: &NodeSpec) -> u64 {
    (spec.memory_mib + PROCESS_OVERHEAD_MIB) * MIB
}

/// The limits of the node `spec`, under a name no other node on this computer has.
pub fn limits_of(spec: &NodeSpec) -> Limits {
    Limits {
        name: format!("{}-{}", spec.id, std::process::id()),
        memory_bytes: memory_limit_bytes(spec),
        cpu_percent: spec.cpu_percent,
    }
}

#[cfg(test)]
mod tests {
    use super::super::Preset;
    use super::*;

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
