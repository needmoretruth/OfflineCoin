//! Orders about coins under a beta lock, and wallet orders while no node runs. Each ends in a
//! notice, and a spend that waits for another signer stays where a person can copy it.
use super::Service;
use crate::core::commands::LockOrder;
use crate::core::wallet_orders::{carry_in_folder, WalletOrder};

impl Service {
    pub(super) fn lock_order(&mut self, order: LockOrder) {
        let (key, detail, good) = self.host.carry_lock(order);
        self.say(key, detail, good);
    }

    /// A wallet order before the first run's questions are done, such as seed words from another
    /// computer: it works on the wallet's files, which the node takes when it starts.
    pub(super) fn wallet_in_folder(&mut self, order: WalletOrder) {
        let (key, detail, good) = carry_in_folder(self.paths.wallet(), order);
        self.say(key, detail, good);
    }
}
