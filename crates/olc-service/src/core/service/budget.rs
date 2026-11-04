//! The person's limit kept in force for every miner this computer runs. Each turn the parts are
//! worked out again and only what changed is told: this app's cap at once, an added miner's cap and
//! threads as one line on its input. A miner about to start or to switch on is counted first, so
//! the others are lowered before it hashes and the sum never goes over.
use super::super::pool_plan::Who;
use super::Service;
use crate::local::{Budget, Holding, NodeSpec};
use crate::settings::standard_threads;

/// What an added miner was last told: its process, its cap and its threads.
pub(super) type Told = (u32, u64, usize);

impl Service {
    /// True when the added miner `spec` hashes or is about to: switched on or working in a pool,
    /// and able to mine at all.
    fn hashes(&self, spec: &NodeSpec, starting: Option<u32>) -> bool {
        let running = self.local.pid(spec.id).is_some() || starting == Some(spec.id);
        let pooled = self.pool.view().link_of(Who::Node(spec.id)).is_some();
        running && spec.holding != Holding::Headers && (spec.mining || pooled)
    }

    /// The parts as they stand, with the miner `starting` counted as if it ran already.
    pub(super) fn budget_with(&self, starting: Option<u32>) -> Budget {
        let threads = self.settings.threads(standard_threads());
        let asked: Vec<(u32, u64)> =
            self.settings.nodes.iter().filter(|spec| self.hashes(spec, starting)).map(|spec| (spec.id, spec.hash_cap)).collect();
        Budget::share(self.settings.hash_cap, self.full_rate, threads, &asked)
    }

    /// Holds this app and every added miner that runs to its part and to the thread cap. A miner
    /// that is switched off is told what it would start at, so switching it on never overshoots.
    pub(super) fn keep_budget(&mut self, budget: &Budget) {
        // Making blocks on order lifts the cap for its run and puts back an older one when it
        // ends, so the cap is set again after.
        if self.maker.busy() {
            self.own_cap = None;
        } else {
            let own = if budget.unbounded() { 0 } else { budget.own.max(1) };
            if self.own_cap != Some(own) {
                self.host.set_cap(own);
                self.own_cap = Some(own);
            }
        }
        self.local.set_threads(budget.threads);
        for spec in self.settings.nodes.clone() {
            let Some(pid) = self.local.pid(spec.id) else {
                self.parts_told.remove(&spec.id);
                continue;
            };
            let cap = budget.part_of(spec.id).unwrap_or(budget.next).max(1);
            let was = self.parts_told.get(&spec.id).copied();
            if was.is_none_or(|(known, said, _)| known != pid || said != cap) {
                let _ = self.local.send(spec.id, &format!("limit {cap}"));
            }
            if was.is_none_or(|(known, _, threads)| known != pid || threads != budget.threads) {
                let _ = self.local.send(spec.id, &format!("threads {}", budget.threads));
            }
            self.parts_told.insert(spec.id, (pid, cap, budget.threads));
        }
    }

    /// Works the parts out again and holds every miner to them.
    pub(super) fn share_budget(&mut self) {
        let budget = self.budget_with(None);
        self.keep_budget(&budget);
    }

    /// Starts an added miner at its part, after the others were lowered to make room for it.
    pub(super) fn start_in_budget(&mut self, spec: &NodeSpec) {
        let budget = self.budget_with(Some(spec.id));
        self.keep_budget(&budget);
        let part = budget.part_of(spec.id).unwrap_or(budget.next).max(1);
        let held = NodeSpec { hash_cap: part, ..spec.clone() };
        self.start_node(&held);
        if let Some(pid) = self.local.pid(spec.id) {
            self.parts_told.insert(spec.id, (pid, part, budget.threads));
        }
    }
}
