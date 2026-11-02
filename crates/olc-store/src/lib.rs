//! A node's files on a computer, shared by the command line and the desktop app.
pub mod coin_file;
mod files;
mod hold;
mod store;
mod wallet_file;

pub use files::{keep_in, Segments};
pub use hold::{hold_folder, node_runs_on, FolderHold, NODE_ON_FOLDER};
pub use store::{load_headers, open_chain, replace_file, save_headers, NodeSettings};
pub use wallet_file::{
    back_up, format_secret, load_wallet, new_secret, parse_secret, restore, save_wallet,
    NEEDS_PASSWORD, NOT_A_BACKUP, WRONG_PASSWORD,
};
