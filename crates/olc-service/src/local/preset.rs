//! Starting numbers for a node process. They only fill the form: every one stays editable.
use super::{Holding, NodeSpec, WalletChoice};
use serde::{Deserialize, Serialize};

/// What a node process takes beyond its chain data, for code, stacks and the allocator. A release
/// build holds about 6.5 MiB resident with an empty chain, so this leaves it a little room.
pub const PROCESS_OVERHEAD_MIB: u64 = 8;

/// The blocks a node keeps when it keeps only recent ones: a day at 30 seconds.
const DEFAULT_RECENT: u64 = 2_880;

/// Measured on an ESP32-S3 at full rate.
const ESP32_RATE: u64 = 15_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Preset {
    /// A miner of this computer: no hash cap, a core of its own, room for a whole chain.
    #[default]
    Computer,
    /// N16R8: 16 MB flash, 8 MB PSRAM.
    Esp32S3,
    /// Raspberry Pi 4 Model B 4 GB, four cores. No hash figure: the CPU share decides the rate.
    RaspberryPi4,
    Custom,
}

impl Preset {
    pub const ALL: [Self; 4] = [Self::Computer, Self::Esp32S3, Self::RaspberryPi4, Self::Custom];

    /// True for the presets that copy a board's numbers.
    pub fn is_board(self) -> bool {
        matches!(self, Self::Esp32S3 | Self::RaspberryPi4)
    }

    pub fn spec(self, id: u32) -> NodeSpec {
        let computer = NodeSpec {
            id,
            name: format!("node-{id}"),
            preset: self,
            hash_cap: 0,
            memory_mib: 1_024,
            cpu_percent: 0,
            // Room for a whole chain: blocks at OLC's size limit come to about 4 GiB a year and
            // ordinary ones to far less. Files grow only as blocks come, and a first opening
            // reads only what was written, so a large room opens at once.
            disk_mib: 8_192,
            wallet: WalletChoice::Own,
            joined: true,
            listen_lan: false,
            mining: true,
            holding: Holding::Everything,
            recent: DEFAULT_RECENT,
        };
        match self {
            Self::Computer => computer,
            Self::Esp32S3 => NodeSpec { hash_cap: ESP32_RATE, memory_mib: 8, disk_mib: 16, ..computer },
            // A form to fill in. It does not mine until someone says so, as in Bitcoin Core.
            Self::Custom => NodeSpec { mining: false, ..computer },
            Self::RaspberryPi4 => NodeSpec {
                mining: false,
                memory_mib: 4_096,
                cpu_percent: 400,
                disk_mib: 32_768,
                ..computer
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_miner_is_sized_for_a_computer() {
        let spec = Preset::default().spec(0);
        assert_eq!(spec.hash_cap, 0, "a computer's miner takes an equal part of this computer's limit");
        assert!(spec.memory_mib >= 256);
        assert!(spec.mining);
        assert!(spec.disk_mib >= 8_192, "a new miner keeps a whole chain");
        assert_eq!(Preset::Esp32S3.spec(0).hash_cap, 15_000);
    }
}
