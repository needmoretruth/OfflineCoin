//! What the service hears from the network, boards and its own threads.
use super::Service;
use crate::core::host::{HeardFrom, PayError};
use crate::discovery::ScanPlan;
use olc_node::Hello;
use olc_wallet::WalletError;

impl Service {
    pub(super) fn plan_scan(&self) {
        let mut addresses = self.found.addresses();
        addresses.extend(self.settings.added.iter().cloned());
        addresses.extend(self.settings.book.iter().cloned());
        addresses.extend(self.settings.peers.iter().cloned());
        self.scanner.set_plan(ScanPlan {
            lan: self.settings.scan_lan,
            ports: self.settings.scan_ports.clone(),
            addresses,
        });
    }

    fn take_answer(&mut self, address: &str, hello: Hello) {
        for (passed, via) in self.found.heard(address, hello) {
            self.found.passed_on(&passed, &via);
            self.settings.remember(&passed);
        }
    }

    /// Takes in everything heard since the last round.
    pub(super) fn listen(&mut self) {
        let before = self.found.addresses().len();
        for (address, hello) in self.scanner.take_found() {
            self.take_answer(&address, hello);
        }
        self.scanner.take_silent();
        for note in self.host.take_radio_notes() {
            self.say("notice.radio_failed", note, false);
        }
        for (from, hello) in self.host.take_heard() {
            match from {
                HeardFrom::Network(ip) if hello.card.port != 0 => {
                    self.take_answer(&format!("{ip}:{}", hello.card.port), hello);
                }
                HeardFrom::Network(_) => {}
                HeardFrom::Board => self.found.board_said(hello),
            }
        }
        for (from, hello) in self.shout.take_heard() {
            if hello.card.port != 0 {
                self.take_answer(&format!("{}:{}", from.ip(), hello.card.port), hello);
            }
        }
        self.found.forget_old();
        if self.found.addresses().len() != before {
            self.plan_scan();
        }
        self.host.set_known(self.found.reachable());
        let shout = self.settings.broadcast.then(|| self.own_hello()).flatten();
        self.shout.set_me(shout);
        let late = self.late.lock().map(|mut late| std::mem::take(&mut *late));
        for (key, detail, good) in late.unwrap_or_default() {
            self.say(key, detail, good);
        }
        for (id, answer) in self.local.take_answers() {
            self.pool.take_node_shares(id, &answer.shares);
            match (answer.paid, answer.error) {
                (Some(_), _) => self.say("notice.node_paid", id.to_string(), true),
                (_, Some(error)) if error == format!("{:?}", PayError::Wallet(WalletError::Refused)) => {
                    self.say("notice.pay_refused", String::new(), false);
                }
                (_, Some(error)) => self.say("notice.pay_failed", error, false),
                _ => {}
            }
        }
        self.local.reap();
        self.pool.step(&self.host, &mut self.local, self.fleet.as_ref());
    }

    fn own_hello(&self) -> Option<Hello> {
        let runner = self.host.runner.try_lock().ok()?;
        let chain = runner.engine.chain();
        Some(Hello {
            coin: chain.config().clone(),
            tip: chain.tip(),
            height: chain.height(),
            work: chain.total_work(),
            address: runner.engine.address(),
            card: runner.card.clone(),
        })
    }
}
