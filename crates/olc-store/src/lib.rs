//! A node's files on a computer, shared by the command line and the desktop app.
pub mod amount;
pub mod coin_fields;
pub mod coin_file;
mod files;
mod hold;
mod sealed;
mod store;
mod wallet_file;

pub use sealed::{
    back_up_locked, change_password, open_wallet, save_open, seal_wallet, sealed_words, Seal, SealedWords,
    ALREADY_SEALED, EMPTY_PASSWORD, NOT_SEALED, WALLET_LOCKED, WRONG_WALLET_PASSWORD,
};

pub use files::{keep_in, Segments};
pub use hold::{hold_folder, hold_wallet, node_runs_on, FolderHold, WalletHold, NODE_ON_FOLDER};
pub use store::{load_headers, open_chain, replace_file, save_headers, NodeSettings};
pub use wallet_file::{
    back_up, format_secret, load_wallet, new_secret, new_words, parse_secret, restore, save_wallet,
    NEEDS_PASSWORD, NOT_A_BACKUP, WRONG_PASSWORD,
};
