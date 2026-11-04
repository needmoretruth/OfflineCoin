//! One look at every node: what each says now, and what happened since the last look, as events
//! the window draws once: blocks found, shares and payouts, payments sent, settled and undone,
//! and blocks replaced by a chain with more work.
use super::*;

/// Replaced blocks kept for the chain strip.
const KEPT_REPLACED: usize = 64;

impl Keeper {
    pub(super) fn look(&mut self) {
        let Some(supervisor) = &mut self.supervisor else {
            return;
        };
        supervisor.reap();
        let answers = supervisor.take_answers();
        for member in &mut self.members {
            let Some(spec) = self.specs.get(&member.id) else {
                continue;
            };
            let view = supervisor.view(spec);
            member.pid = view.pid;
            member.ended = view.ended;
            if view.status.is_some() {
                member.status = view.status;
            }
        }
        for (id, status) in answers {
            self.answered(id, &status);
        }
        self.blocks();
        self.pools();
        self.incoming();
    }

    /// A payment a participant was asked to make, signed and sent, or refused.
    fn answered(&mut self, id: u32, status: &ChildStatus) {
        let Some(asked) = self.asked.get_mut(&id).and_then(VecDeque::pop_front) else {
            return;
        };
        if status.paid.is_some() {
            self.event(What::Paid { from: id, to: asked.to, amount: asked.amount });
        } else if let Some(member) = self.member_mut(id) {
            member.waits = Some("net.wait_coins");
        }
    }

    /// New blocks on any chain, each credited to the participant its reward pays, and blocks a
    /// node no longer holds where its chain now has others.
    fn blocks(&mut self) {
        let owners: BTreeMap<String, u32> =
            self.members.iter().filter_map(|member| Some((member.address()?.to_string(), member.id))).collect();
        let mut found: Vec<(u64, String, Option<u32>, u64)> = Vec::new();
        let mut replaced: Vec<(u32, Vec<String>)> = Vec::new();
        for member in &self.members {
            let Some(status) = &member.status else {
                continue;
            };
            for block in status.chain.iter().filter(|block| block.height > 0) {
                if !self.seen_blocks.contains(&block.id) && !found.iter().any(|(_, id, ..)| id == &block.id) {
                    let finder = owners.get(&block.miner).copied();
                    found.push((block.height, block.id.clone(), finder, block.txs.len() as u64));
                }
            }
            if let Some(before) = self.chains.get(&member.id) {
                let lowest = status.chain.first().map_or(u64::MAX, |block| block.height);
                let lost: Vec<String> = before
                    .iter()
                    .filter(|old| old.height >= lowest && old.height > 0)
                    .filter(|old| !status.chain.iter().any(|new| new.id == old.id))
                    .map(|old| old.id.clone())
                    .collect();
                if !lost.is_empty() {
                    replaced.push((member.id, lost));
                }
            }
        }
        found.sort();
        for (height, id, finder, payments) in found {
            self.seen_blocks.insert(id.clone());
            self.top = self.top.max(height);
            self.event(What::Block { finder, height, id, payments });
        }
        for (at, lost) in replaced {
            self.event(What::Replaced { at, blocks: lost.len() as u64 });
            for id in lost {
                if !self.replaced.contains(&id) {
                    self.replaced.push_back(id);
                }
            }
        }
        while self.replaced.len() > KEPT_REPLACED {
            self.replaced.pop_front();
        }
        self.chains = self
            .members
            .iter()
            .filter_map(|member| Some((member.id, member.status.as_ref()?.chain.clone())))
            .collect();
    }

    /// Shares each pool took from each of its miners since the last look, and what it paid them.
    fn pools(&mut self) {
        let by_address: BTreeMap<String, u32> =
            self.members.iter().filter_map(|member| Some((member.address()?.to_string(), member.id))).collect();
        let mut news = Vec::new();
        for pool in self.members.iter().filter(|member| member.kind == Kind::Pool) {
            let Some(line) = pool.status.as_ref().and_then(|status| status.lab.as_ref()) else {
                continue;
            };
            for worker in &line.workers {
                let Some(miner) = by_address.get(&worker.address).copied() else {
                    continue;
                };
                news.push((pool.id, miner, worker.accepted, worker.paid));
            }
        }
        for (pool, miner, accepted, paid) in news {
            let before = self.accepted.insert((pool, miner), accepted).unwrap_or(0);
            if accepted > before {
                self.event(What::Shares { from: miner, to: pool, count: accepted - before });
            }
            let before = self.payouts.insert((pool, miner), paid).unwrap_or(0);
            if paid > before {
                self.event(What::Payout { from: pool, to: miner, amount: paid - before });
            }
        }
    }

    /// Payments each receiver holds: settled under enough blocks, or undone.
    fn incoming(&mut self) {
        let mut news = Vec::new();
        for member in &self.members {
            let Some(status) = &member.status else {
                continue;
            };
            let Some(line) = &status.lab else {
                continue;
            };
            for payment in &line.incoming {
                let key = (member.id, payment.id.clone());
                let deep = payment.height.is_some_and(|height| status.height + 1 >= height + SETTLED_AFTER);
                if deep && !payment.gone && !self.settled.contains(&key) {
                    news.push((key, payment.amount, true));
                } else if payment.gone && !self.undone.contains(&key) {
                    news.push((key, payment.amount, false));
                }
            }
        }
        for (key, amount, settled) in news {
            let at = key.0;
            if settled {
                self.settled.insert(key);
                self.event(What::Settled { at, amount });
            } else {
                self.undone.insert(key);
                self.event(What::Undone { at, amount });
            }
        }
    }
}
