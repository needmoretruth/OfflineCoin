//! What the service hears from the network, boards and its own threads.
use super::Service;
use crate::core::console::known_key;
use crate::core::host::HeardFrom;
use crate::core::pool_plan::PoolOrder;
use crate::core::pool_run::PoolCx;
use crate::core::pool_view::listing as pool_listing;
use crate::discovery::ScanPlan;
use olc_node::Hello;
use std::time::{Duration, Instant};

/// Long enough to leave a radio alone, short enough that a board's move shows soon after.
const ASK_BOARDS_EVERY: Duration = Duration::from_secs(20);

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
                HeardFrom::Board(link) => self.found.board_said_on(hello, link),
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
                // The node names its failure by the key of a sentence, chosen from the error itself.
                (_, Some(key)) => {
                    let key = known_key(&key).unwrap_or("notice.pay_failed");
                    self.say(key, answer.detail, false);
                }
                _ => {}
            }
        }
        self.ask_boards();
        self.local.reap();
        let hellos = self.found.boards_on_links();
        let mut cx = PoolCx {
            host: &self.host,
            local: &mut self.local,
            fleet: self.fleet.as_ref(),
            specs: &self.settings.nodes,
            hellos: &hellos,
            settings: &mut self.settings.pool,
        };
        self.pool.step(&mut cx);
    }

    /// What a person asked of the pool. The choices it leaves are kept for the next start.
    pub(super) fn pool_order(&mut self, order: PoolOrder) {
        let hellos = self.found.boards_on_links();
        let mut cx = PoolCx {
            host: &self.host,
            local: &mut self.local,
            fleet: self.fleet.as_ref(),
            specs: &self.settings.nodes,
            hellos: &hellos,
            settings: &mut self.settings.pool,
        };
        let show = order == PoolOrder::Show;
        let done = self.pool.order(order, &mut cx);
        drop(hellos);
        if show {
            self.say("pool.list", pool_listing(&self.pool.view()), true);
            return;
        }
        self.save();
        if !done {
            self.say("notice.pool_failed", self.pool.failure(), false);
        }
    }

    /// A board's rules change with no sign in its beacon, so each is asked now and then.
    fn ask_boards(&mut self) {
        if self.asked_boards.is_some_and(|asked| asked.elapsed() < ASK_BOARDS_EVERY) {
            return;
        }
        self.asked_boards = Some(Instant::now());
        let rows = self.fleet.as_ref().map(|fleet| fleet.board_rows()).unwrap_or_default();
        for row in rows.iter().filter(|row| row.seen.online && row.job.is_none()) {
            self.read_state(row.seen.id);
        }
    }

    fn own_hello(&self) -> Option<Hello> {
        if self.host.opening() {
            return None;
        }
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
