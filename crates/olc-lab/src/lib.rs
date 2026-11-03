//! Experiments that use a board only through the commands any owner has. They live here, outside
//! the coin: a miner with most of the hash power can do this with ordinary software, and nothing
//! in the coin or the node exists for it. The same experiments run on miners on this computer,
//! which answer the lines a board answers.
mod attack;
mod split;
mod steps;

pub use attack::{attack_with, AttackOptions};
pub use olc_board::journal::Journal;
pub use split::{split_with, SplitOptions};
pub use steps::{Outcome, Step, Watch};

use olc_board::{Args, BoardLink, Extra, Say};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

pub struct Lab;

impl Extra for Lab {
    fn command(&self) -> &'static str {
        "attack"
    }

    fn usage(&self) -> &'static str {
        "  olc attack  --attacker PORT --victim PORT [--amount 10] [--confirmations 2]
              [--victim-limit N] [--timeout 1800]
"
    }

    fn run(&self, args: &Args) -> Result<(), String> {
        attack::run(args)
    }

    fn run_on(
        &self,
        boards: Vec<Box<dyn BoardLink>>,
        args: &Args,
        say: Say,
        stop: Arc<AtomicBool>,
    ) -> Result<(), String> {
        attack::run_on(boards, args, say, stop)
    }
}
