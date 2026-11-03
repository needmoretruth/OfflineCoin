//! Pool work for a board this computer reaches only through another board, as
//! `olc_control::lend` carries it. The work goes out with the next pump; nothing waits for it.
use crate::manager::Manager;
use olc_control::lend::Address;
use olc_control::BoardId;

impl Manager {
    /// Signs pool work for the board that mines to `worker` and sends it to `relay`, the board
    /// that hears it. `None` stops the work.
    pub fn lend(&self, relay: BoardId, worker: Address, work: Option<(Vec<u8>, u64)>) -> bool {
        let Ok(mut fleet) = self.fleet.lock() else {
            return false;
        };
        fleet.lend(relay, worker, work).is_ok()
    }

    /// What the board that mines to `worker` found for lent work since the last call.
    pub fn take_lent_shares(&self, worker: &Address) -> Vec<Vec<u8>> {
        let Ok(mut fleet) = self.fleet.lock() else {
            return Vec::new();
        };
        fleet.take_shares(worker)
    }
}
