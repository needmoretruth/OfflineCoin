//! The work behind the window. The screen reads one [`Snapshot`] and sends [`Command`]s, so a slow
//! board or a busy miner can never freeze it.
mod board_action;
mod folder;
mod board_lines;
mod terminal;
mod words;
mod coins;
mod commands;
mod console;
mod explorer;
#[cfg(test)]
mod forged;
mod found;
mod history;
mod host;
mod host_parts;
mod ops;
#[cfg(test)]
mod ops_tests;
mod pool_run;
pub mod radios;
mod returning;
mod rows;
mod service;
mod snapshot;

pub use board_action::BoardAction;
pub use folder::Folder;
pub use terminal::Terminal;
pub use words::{run_line, run_words, usage, Desk, Word, WORDS};
pub use ops::Book;
pub use olc_store::coin_file::{new_coin, NewCoin};
pub use commands::{Command, Personal, StartCoin};
pub use explorer::{BlockRow, Explored, NoteRow, TxRow};
pub use history::{Activity, ActivityKind};
pub use coins::{read_coin_file, write_coin_file, coin_folder, format_coins, group_thousands, parse_coins, save_coin};
pub use host_parts::{full_rate, unix_ms};
pub use host::{HeardFrom, Host, PayError, Plan};
pub use pool_run::{shares_in, PoolRow, PoolRun, PoolView, Worker};
pub use rows::device_kind;
pub use service::{short_hex, Core, CoreLaunch};
pub use snapshot::{BoardView, ExperimentView, KeyView, NodeView, Notice, ScanView, Snapshot};
