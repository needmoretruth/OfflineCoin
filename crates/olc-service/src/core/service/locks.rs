//! Orders about coins under a beta lock. Each ends in a notice, and a spend that waits for another
//! signer stays where a person can copy it.
use super::Service;
use crate::core::commands::LockOrder;
use crate::core::host_locks::{fault_of_lock, name_of_coin, LockDone};

impl Service {
    pub(super) fn lock_order(&mut self, order: LockOrder) {
        let done = match order {
            LockOrder::Pay { words, amount } => match self.host.pay_lock(&words, &amount) {
                Ok((id, amount)) => {
                    let coin = olc_core::OutPoint { txid: id, index: 0 };
                    self.say("notice.lock_paid", format!("{amount} {}", name_of_coin(&coin)), true);
                    return;
                }
                Err(failure) => Err(failure),
            },
            LockOrder::Spend { coin, to, unlock } => self.host.spend_lock(&coin, &to, &unlock),
            LockOrder::Sign { text } => self.host.sign_lock(&text),
        };
        match done {
            Ok(LockDone::Sent(id)) => {
                let id: String = id.iter().map(|byte| format!("{byte:02x}")).collect();
                self.host.set_half_spend(None);
                self.say("notice.lock_sent", id, true);
            }
            Ok(LockDone::Waiting { text, who }) => {
                self.host.set_half_spend(Some((text.clone(), who)));
                self.say("notice.lock_waiting", text, true);
            }
            Err(failure) => {
                let (key, detail) = fault_of_lock(&failure);
                self.say(key, detail, false);
            }
        }
    }
}
