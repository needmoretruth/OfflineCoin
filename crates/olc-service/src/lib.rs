//! Everything OLC Core does, with no screen. A window reads a [`core::Snapshot`] and sends
//! [`core::Command`]s, so another kind of window would change nothing here.
pub mod core;
pub mod discovery;
pub mod files;
pub mod fleet;
pub mod instance;
pub mod local;
pub mod qr;
pub mod settings;

pub use olc_platform::platform;
