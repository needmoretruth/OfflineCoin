//! OLC Core's window. `olc-service` does the work and knows nothing of the screen. `ui` and
//! `pages` only read a snapshot and send commands.
pub mod app;
pub mod i18n;
pub mod pages;
#[cfg(test)]
mod screens;
pub mod ui;

pub use olc_service::{core, discovery, fleet, instance, local, settings};
