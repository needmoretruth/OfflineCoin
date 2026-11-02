//! Orders about coins under a beta lock. Each ends in a notice, and a spend that waits for another
//! signer stays where a person can copy it.
use super::Service;
use crate::core::commands::LockOrder;

impl Service {
    pub(super) fn lock_order(&mut self, order: LockOrder) {
        let (key, detail, good) = self.host.carry_lock(order);
        self.say(key, detail, good);
    }
}
