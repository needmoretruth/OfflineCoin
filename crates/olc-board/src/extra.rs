//! An optional program built on the `olc` command. The coin and the node know nothing of these.
use crate::{Args, BoardLink};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// Where a run's lines go: the terminal, or the app's page.
pub type Say = Arc<dyn Fn(String) + Send + Sync>;

pub trait Extra: Sync {
    /// The word after `olc` that starts it.
    fn command(&self) -> &'static str;
    /// Its lines in the help text.
    fn usage(&self) -> &'static str;
    fn run(&self, args: &Args) -> Result<(), String>;
    /// A script for the app's page, which adds its own panel.
    fn script(&self) -> &'static str;
    /// Runs from the app, on a thread of its own, on the boards the page chose. Blocks until done.
    fn run_on(
        &self,
        boards: Vec<Box<dyn BoardLink>>,
        args: &Args,
        say: Say,
        stop: Arc<AtomicBool>,
    ) -> Result<(), String>;
}
