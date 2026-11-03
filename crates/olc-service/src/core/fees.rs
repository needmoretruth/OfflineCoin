//! What a fee buys now: how many blocks a payment at a rate waits, read from the pool the way a
//! miner here fills a block, best rate first. Bitcoin Core's estimatesmartfee reads past blocks;
//! blocks here come every few seconds, so the pool itself says more.
use olc_core::{pays_more, Chain, Tx};

/// The pool as fee levels: every waiting payment's fee and size, best rate first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FeeLevels {
    /// The lowest fee the coin takes, whatever the size.
    pub min_fee: u64,
    /// Bytes one block has for payments.
    pub room: u64,
    pub block_ms: u64,
    queue: Vec<(u64, u64)>,
}

/// A rate in whole units a byte that pays at least `fee` over `size`.
fn rate_of(fee: u64, size: u64) -> u64 {
    fee.div_ceil(size.max(1))
}

impl FeeLevels {
    pub fn read(chain: &Chain, pool: &[Tx]) -> Self {
        let mut queue: Vec<(u64, u64)> = chain.waiting(pool).iter().map(|entry| (entry.fee, entry.size)).collect();
        queue.sort_by(|a, b| match (pays_more(a.0, a.1, b.0, b.1), pays_more(b.0, b.1, a.0, a.1)) {
            (true, _) => std::cmp::Ordering::Less,
            (_, true) => std::cmp::Ordering::Greater,
            _ => std::cmp::Ordering::Equal,
        });
        let next = chain.height() + 1;
        let rules = chain.config().rules_at(next);
        Self { min_fee: rules.min_fee, room: chain.room_for_payments(0).max(1), block_ms: rules.block_time_ms, queue }
    }

    /// The lowest rate that gets into one of the next `blocks` blocks, if they fill as the pool
    /// stands. 0 when they have room for every payment waiting: the lowest fee is enough then.
    pub fn rate_within(&self, blocks: u64) -> u64 {
        let room = self.room.saturating_mul(blocks.max(1));
        let mut taken = 0u64;
        for (fee, size) in &self.queue {
            taken = taken.saturating_add(*size);
            if taken > room {
                return rate_of(*fee, *size) + 1;
            }
        }
        0
    }

    /// About how many blocks a payment of `size` bytes paying `fee` waits: one, and one more for
    /// each block of payments that pay a better rate.
    pub fn blocks_for(&self, fee: u64, size: u64) -> u64 {
        let ahead: u64 = self.queue.iter().filter(|(other, other_size)| !pays_more(fee, size, *other, *other_size)).map(|(_, size)| size).sum();
        1 + (ahead + size) / self.room
    }

    pub fn waiting(&self) -> usize {
        self.queue.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn levels(queue: Vec<(u64, u64)>) -> FeeLevels {
        FeeLevels { min_fee: 100, room: 1_000, block_ms: 5_000, queue }
    }

    #[test]
    fn an_empty_pool_takes_the_lowest_fee_into_the_next_block() {
        let empty = levels(Vec::new());
        assert_eq!(empty.rate_within(1), 0);
        assert_eq!(empty.blocks_for(100, 200), 1);
    }

    #[test]
    fn a_full_pool_asks_more_than_the_last_payment_that_fits() {
        // Three blocks' worth: 600 bytes at 10 a byte, then 1,000 at 5, then 1,400 at 1.
        let full = levels(vec![(6_000, 600), (5_000, 1_000), (1_400, 1_400)]);
        assert_eq!(full.rate_within(1), 6, "the payment at 5 a byte no longer fits the next block");
        assert_eq!(full.rate_within(2), 2);
        assert_eq!(full.rate_within(3), 0);
        assert_eq!(full.blocks_for(6 * 200, 200), 1);
        assert_eq!(full.blocks_for(200, 200), 4, "three full blocks pay as much or more");
    }
}
