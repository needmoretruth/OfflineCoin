//! How many more participants this computer carries. Each is a node process held to
//! [`MEMORY_MIB`] of chain data plus the process itself, and a miner keeps a core busy for the
//! share of time its cap asks. The Lab stops adding before memory runs low or the cores crowd.
use super::PROCESS_OVERHEAD_MIB;

/// Memory each node of the Lab may use for its chain: the Lab's chains are small.
pub const MEMORY_MIB: u64 = 64;
pub const DISK_MIB: u64 = 64;
/// Memory kept free for the rest of the computer.
const KEEP_FREE_MIB: u64 = 1_024;
/// Nodes a core carries side by side: they sleep most of the time.
const PER_CORE: u64 = 4;
/// Never more than this many, so the map stays readable.
pub const MOST: u64 = 24;
const MIB: u64 = 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Limit {
    #[default]
    Map,
    Memory,
    Cores,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Room {
    /// Participants running now.
    pub running: u64,
    /// How many more can be added.
    pub more: u64,
    pub free_mib: u64,
    pub cores: u64,
    /// What sets the limit.
    pub limit: Limit,
}

impl Room {
    pub fn measure(running: usize) -> Self {
        let free_mib = available_mib();
        let cores = olc_platform::physical_cores() as u64;
        Self::from_parts(running as u64, free_mib, cores)
    }

    pub fn from_parts(running: u64, free_mib: u64, cores: u64) -> Self {
        let each = MEMORY_MIB + PROCESS_OVERHEAD_MIB;
        // What is free now already leaves out the running nodes.
        let by_memory = running + free_mib.saturating_sub(KEEP_FREE_MIB) / each;
        let by_cores = cores.max(1) * PER_CORE;
        let (most, limit) = [(MOST, Limit::Map), (by_memory, Limit::Memory), (by_cores, Limit::Cores)]
            .into_iter()
            .min_by_key(|(most, _)| *most)
            .unwrap_or((MOST, Limit::Map));
        Self { running, more: most.saturating_sub(running), free_mib, cores, limit }
    }

    pub fn full(&self) -> bool {
        self.more == 0
    }
}

fn available_mib() -> u64 {
    match sysinfo::System::new() {
        Ok(mut system) => {
            system.refresh_memory();
            system.available_memory() / MIB
        }
        // Unknown: the cores and the map's own limit still hold.
        Err(_) => u64::MAX / MIB,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_smallest_limit_wins_and_says_which() {
        let roomy = Room::from_parts(0, 30_000, 6);
        assert_eq!((roomy.more, roomy.limit), (MOST, Limit::Map));
        let tight = Room::from_parts(3, 1_024 + 72 * 2, 6);
        assert_eq!((tight.more, tight.limit), (2, Limit::Memory));
        let few = Room::from_parts(5, 30_000, 2);
        assert_eq!((few.more, few.limit), (3, Limit::Cores));
        assert!(Room::from_parts(8, 30_000, 2).full());
    }
}
