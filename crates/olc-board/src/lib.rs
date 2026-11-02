//! A computer's side of a running board: lines to send, answers to read, and the seam where
//! optional programs plug into the `olc` command.
mod args;
pub mod autoupdate;
mod board;
pub mod burst;
mod cable;
pub mod carried;
pub mod clearance;
mod comeback;
pub mod control_key;
mod extra;
pub mod fleet;
pub mod install;
pub mod journal;
pub mod manager;
pub mod pins;
mod ports;
pub mod remote;
pub mod rom;
mod state;

pub use ports::board_ports;
pub use args::Args;
pub use board::{on_off, Board, BoardLink, Place, Status, UsbBoard};
pub use state::BoardState;
pub use extra::{Extra, Say};
pub use fleet::{Connection, Fleet, Seen};
pub use manager::Manager;
pub use remote::{RemoteBoard, SharedFleet};
