//! The place for an Ethereum-level language. Implement `check` here to bring it to life.
use crate::OMEGA;
use olc_core::{LockLanguage, SpendContext};

pub struct Omega;

impl LockLanguage for Omega {
    fn id(&self) -> u8 {
        OMEGA
    }

    fn name(&self) -> &'static str {
        "omega"
    }

    fn check(&self, _lock: &[u8], _unlock: &[u8], _context: &SpendContext) -> bool {
        false
    }
}
