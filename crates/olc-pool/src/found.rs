//! The node's side of the pool: new templates, the blocks the pool found, and paying for them once
//! their coins can be spent, with one ordinary payment per block.
use crate::ledger::split;
use crate::pool::{BlockState, Pool};
use olc_core::{Address, Block, Hash};
use olc_node::Engine;
use std::collections::BTreeMap;

impl Pool {
    /// Builds a new job when the chain moved or the last one is old. False when this coin's
    /// blocks cannot be carried by Stratum.
    pub fn refresh(&mut self, engine: &Engine, now_ms: u64) -> bool {
        if !self.wants_template(engine.chain().tip(), now_ms) {
            return true;
        }
        let block = engine.work_template(&self.payout, now_ms, &[]);
        self.take_template(block, now_ms)
    }

    /// Blocks shares sealed, for the node to add.
    pub fn take_blocks(&mut self) -> Vec<Block> {
        std::mem::take(&mut self.outbox)
    }

    /// What the node said about a block it was given. A block it refused pays for nothing, and its
    /// round's work counts toward the next one.
    pub fn block_added(&mut self, id: Hash, added: bool) {
        let Some(found) = self.found.iter_mut().find(|found| found.id == id && found.state == BlockState::Sent) else {
            return;
        };
        if added {
            found.state = BlockState::Waiting;
            return;
        }
        found.state = BlockState::Lost;
        let credits = std::mem::take(&mut found.credits);
        self.ledger.reopen(credits);
        // The node did not take it, so the next share that seals on the same parent may.
        self.sealed_on = None;
    }

    /// Hands out every block's coins once they can be spent, and marks a block that left the
    /// chain lost. Returns the ids of the payments made.
    pub fn pay(&mut self, engine: &mut Engine) -> Vec<Hash> {
        let height = engine.chain().height();
        let mut paid = Vec::new();
        for at in 0..self.found.len() {
            let found = &self.found[at];
            if found.state != BlockState::Waiting || engine.chain().rules().matures_at(found.height) > height {
                continue;
            }
            if engine.chain().id_at(found.height) != Some(found.id) {
                self.found[at].state = BlockState::Lost;
                let credits = std::mem::take(&mut self.found[at].credits);
                self.ledger.reopen(credits);
                continue;
            }
            if let Some(id) = self.pay_block(engine, at) {
                paid.push(id);
            }
        }
        paid
    }

    /// One block's payment: each worker's part to its address, the pool's own parts and its fee
    /// left where the coinbase put them. Waits when the coins are tied up elsewhere.
    fn pay_block(&mut self, engine: &mut Engine, at: usize) -> Option<Hash> {
        let found = &self.found[at];
        let fee = engine.chain().rules().min_fee;
        let payments_of = |shares: &crate::ledger::Split| {
            let mut by_address: BTreeMap<Address, u64> = BTreeMap::new();
            for (name, part) in &shares.parts {
                let address = self.workers.get(name).map_or(self.payout, |stats| stats.address);
                *by_address.entry(address).or_default() += part;
            }
            // The pool's own address already holds its parts: paying itself would only cost a fee.
            let others = by_address.into_iter().filter(|(address, amount)| *address != self.payout && *amount > 0);
            others.collect::<Vec<(Address, u64)>>()
        };
        let mut shares = split(found.value, self.plan.fee_bps, fee, &found.credits);
        let payments = payments_of(&shares);
        if payments.is_empty() {
            // No payment, so no payment's fee: the pool keeps those coins too.
            shares.pool_fee += std::mem::take(&mut shares.payment_fee);
        }
        let id = match payments.is_empty() {
            true => [0; 32],
            false => engine.pay_many(&payments, shares.payment_fee).ok()?,
        };
        for (name, part) in &shares.parts {
            if let Some(stats) = self.workers.get_mut(name) {
                stats.paid = stats.paid.saturating_add(*part);
            }
        }
        let found = &mut self.found[at];
        found.state = BlockState::Paid;
        found.split = Some(shares);
        (!payments.is_empty()).then_some(id)
    }

    /// How each paid block split, newest first, for a person checking the sums: its height, its
    /// value and where every coin went.
    pub fn splits(&self) -> Vec<(u64, u64, crate::ledger::Split)> {
        let paid = self.found.iter().rev().filter_map(|found| Some((found.height, found.value, found.split.clone()?)));
        paid.collect()
    }
}
