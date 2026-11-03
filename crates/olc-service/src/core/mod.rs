//! The work behind the window. The screen reads one [`Snapshot`] and sends [`Command`]s, so a slow
//! board or a busy miner can never freeze it.
mod address_index;
mod board_action;
mod chain_race;
pub mod chain_view;
mod folder;
mod board_lines;
mod terminal;
mod words;
mod words_channels;
mod words_swap;
mod words_wallet;
mod coins;
mod commands;
mod console;
#[cfg(test)]
mod dial_tests;
mod explorer;
mod fees;
#[cfg(test)]
mod forged;
mod found;
mod history;
mod host;
pub mod host_channels;
pub mod host_locks;
mod host_parts;
mod host_wallet;
mod wallet_orders;
mod ops;
#[cfg(test)]
mod ops_tests;
mod pool_run;
pub mod radios;
mod returning;
mod rows;
mod service;
mod snapshot;
pub mod refusals;
pub mod swap;
mod swap_book;
pub mod swap_text;

pub use board_action::BoardAction;
pub use folder::Folder;
pub use terminal::Terminal;
pub use words::{run_line, run_words, usage, Desk, Word, WORDS};
pub use ops::Book;
pub use olc_store::coin_fields::{
    first_fork_height, fork_coin, new_coin, Bad, CoinField, FieldKind, NewCoin, Why, COIN_FIELDS,
};
pub use commands::{ChannelOrder, Command, LockOrder, Personal, StartCoin, SwapOrder};
pub use address_index::AddressMove;
pub use explorer::{BlockDetail, BlockRow, Explored, InputRow, NoteRow, OutputRow, PendingRow, TxRow};
pub use chain_view::{Branch, ChainView, MinedBlock, Miner, Share, SHARE_WINDOW};
pub use history::{Activity, ActivityKind, KEPT, RECENT};
pub use fees::FeeLevels;
pub use coins::{read_coin_file, write_coin_file, coin_folder, format_coins, group_thousands, parse_coins, save_coin};
pub use host_parts::{full_rate, unix_ms};
pub use host_wallet::{SharedView, SpeedQuote, WalletView, Words};
pub use wallet_orders::WalletOrder;
pub use host::{HeardFrom, Host, PayError, Plan};
pub use pool_run::{shares_in, PoolRow, PoolRun, PoolView, Worker};
pub use found::LIVE_WITHIN;
pub use rows::device_kind;
pub use service::{short_hex, Core, CoreLaunch};
pub use snapshot::{BoardView, ExperimentView, KeyView, NodeView, Notice, ScanView, Snapshot, SwapView, SwapsView};
