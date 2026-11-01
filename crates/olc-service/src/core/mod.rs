//! The work behind the window. The screen reads one [`Snapshot`] and sends [`Command`]s, so a slow
//! board or a busy miner can never freeze it.
mod board_lines;
mod coins;
mod commands;
#[cfg(test)]
mod forged;
mod found;
mod history;
mod host;
mod returning;
mod rows;
mod service;
mod snapshot;

pub use commands::{Command, StartCoin};
pub use history::{Activity, ActivityKind};
pub use coins::{coin_folder, format_coins, parse_coins, save_coin};
pub use host::{full_rate, unix_ms, HeardFrom, Host, PayError, Plan};
pub use rows::device_kind;
pub use service::{short_hex, Core, CoreLaunch};
pub use snapshot::{BoardView, ExperimentView, NodeView, Notice, ScanView, Snapshot};
