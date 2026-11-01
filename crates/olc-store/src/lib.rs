//! A node's files on a computer, shared by the command line and the desktop app.
pub mod coin_file;
mod store;

pub use store::{load_or_create_secret, Store};
