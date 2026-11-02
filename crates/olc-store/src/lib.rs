//! A node's files on a computer, shared by the command line and the desktop app.
pub mod amount;
pub mod coin_fields;
pub mod coin_file;
mod files;
mod store;
mod wallet_file;

pub use files::{keep_in, Segments};
pub use store::{load_headers, open_chain, save_headers, NodeSettings};
pub use wallet_file::{
    back_up, format_secret, load_wallet, new_secret, parse_secret, restore, save_wallet,
    NEEDS_PASSWORD, NOT_A_BACKUP, WRONG_PASSWORD,
};
