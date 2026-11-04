//! The chain words a Bitcoin Core user reaches for first: the waiting payments, this wallet's
//! coins, and blocks made now. Making mines for real, on every thread with no cap, until this node
//! made as many blocks as asked, says each one, and then mines as it did before.
use super::super::making::ChainOrder;
use super::super::pool_plan::Who;
use super::Service;

impl Service {
    pub(super) fn chain_order(&mut self, order: ChainOrder) {
        // Work from a pool goes before the node's own blocks, so none of them would be made.
        if matches!(order, ChainOrder::Make(_)) && !self.maker.busy() && self.pool.view().link_of(Who::Own).is_some() {
            self.say("notice.making_in_pool", String::new(), false);
            return;
        }
        let (key, detail, good) = self.maker.order(&self.host, order);
        self.say(key, detail, good);
    }

    /// Each turn: the blocks this node made since the last, and the end.
    pub(super) fn step_making(&mut self) {
        for (key, detail, good) in self.maker.step(&self.host) {
            self.say(key, detail, good);
        }
    }
}
