//! OLCL: four lock languages behind one trait.
//! - alpha: pay to an address. Fixed format.
//! - beta: a small stack language at the level of early Bitcoin Script.
//! - gamma: Bitcoin Script as it runs today, without Taproot. A coin takes it from its upgrade.
//! - omega: reserved for an Ethereum-level language. Nothing can be spent with it yet.
#![no_std]
extern crate alloc;

pub mod alpha;
pub mod beta;
pub mod gamma;
mod gamma_words;
pub mod omega;
pub mod words;

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use olc_core::LockLanguage;

pub const ALPHA: u8 = 0;
pub const BETA: u8 = 1;
pub const OMEGA: u8 = 2;
pub const GAMMA: u8 = 3;

pub fn all() -> Vec<Box<dyn LockLanguage>> {
    vec![
        Box::new(alpha::Alpha),
        Box::new(beta::Beta),
        Box::new(gamma::Gamma),
        Box::new(omega::Omega),
    ]
}
