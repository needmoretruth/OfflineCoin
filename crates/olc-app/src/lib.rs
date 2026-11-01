//! OLC Core: one desktop app that runs a node and a miner, holds a wallet, finds every OLC it can
//! reach and manages boards. `core` does the work and knows nothing of the screen. `ui` and
//! `pages` only read a snapshot and send commands.
pub mod app;
pub mod core;
pub mod discovery;
pub mod fleet;
pub mod i18n;
pub mod instance;
pub mod local;
pub mod pages;
#[cfg(test)]
mod screens;
pub mod settings;
pub mod ui;
