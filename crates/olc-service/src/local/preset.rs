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
    /// N16R8: 16 MB flash, 8 MB PSRAM.
    #[default]
    Esp32S3,
    /// Raspberry Pi 4 Model B 4 GB, four cores. No hash figure: the CPU share decides the rate.
    RaspberryPi4,
    Custom,
}

impl Preset {
    pub const ALL: [Self; 3] = [Self::Esp32S3, Self::RaspberryPi4, Self::Custom];

    pub fn spec(self, id: u32) -> NodeSpec {
        let base = NodeSpec {
            id,
            name: format!("node-{id}"),
            preset: self,
            hash_cap: ESP32_RATE,
            memory_mib: 8,
            cpu_percent: 0,
            disk_mib: 16,
            wallet: WalletChoice::Own,
            joined: true,
            listen_lan: false,
            mining: true,
            holding: Holding::Everything,
            recent: DEFAULT_RECENT,
        };
        match self {
            Self::Esp32S3 => base,
            // Mining is off by default on a computer, as in Bitcoin Core.
            Self::Custom => NodeSpec { mining: false, ..base },
            Self::RaspberryPi4 => NodeSpec {
                mining: false,
                hash_cap: 0,
                memory_mib: 4_096,
                cpu_percent: 400,
                disk_mib: 32_768,
                ..base
            },
        }
    }
}
