//! A computer's side of a running board: lines to send, answers to read, and the seam where
//! optional programs plug into the `olc` command.
mod args;
mod board;
pub mod control_key;
mod extra;
pub mod fleet;
pub mod install;
pub mod remote;

pub use args::Args;
pub use board::{on_off, Board, BoardLink, Place, Status, UsbBoard};
pub use extra::{Extra, Say};
pub use fleet::{Connection, Fleet, Seen};
pub use remote::{RemoteBoard, SharedFleet};
