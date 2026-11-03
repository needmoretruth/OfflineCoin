//! Who earned what. Each share counts the hashes it stands for, its difficulty, so a slow worker
//! on easy shares and a fast one on hard shares are paid by the work they did. A block's coins are
//! split to the last unit: what rounding leaves goes to the largest remainders, never to nobody.
use std::collections::{BTreeMap, VecDeque};

/// How a block's coins are shared out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Payout {
    /// By the work each worker did since the pool's last block (PROP).
    #[default]
    Proportional,
    /// By the work in the last `shares` shares, whenever they came (PPLNS, as Kano's pool pays).
    LastShares { shares: u64 },
}

/// The largest window PPLNS may look back over: each share is a few dozen bytes kept in memory.
pub const MOST_LAST_SHARES: u64 = 1_000_000;
/// The highest fee a pool may take, in hundredths of a percent: 5 %.
pub const MOST_FEE_BPS: u32 = 500;

/// Work by worker name.
pub type Credits = BTreeMap<String, u128>;

#[derive(Default)]
pub struct Ledger {
    payout: Payout,
    /// Work since the last block, for [`Payout::Proportional`].
    round: Credits,
    /// The newest shares, for [`Payout::LastShares`].
    last: VecDeque<(String, u64)>,
}

impl Ledger {
    pub fn new(payout: Payout) -> Self {
        let payout = match payout {
            Payout::LastShares { shares } => Payout::LastShares { shares: shares.clamp(1, MOST_LAST_SHARES) },
            other => other,
        };
        Self { payout, ..Self::default() }
    }

    pub fn payout(&self) -> Payout {
        self.payout
    }

    pub fn add(&mut self, worker: &str, difficulty: u64) {
        match self.payout {
            Payout::Proportional => *self.round.entry(worker.to_string()).or_default() += u128::from(difficulty),
            Payout::LastShares { shares } => {
                self.last.push_back((worker.to_string(), difficulty));
                while self.last.len() as u64 > shares {
                    self.last.pop_front();
                }
            }
        }
    }

    /// The work a block found now pays for. A proportional round starts over.
    pub fn close_round(&mut self) -> Credits {
        match self.payout {
            Payout::Proportional => std::mem::take(&mut self.round),
            Payout::LastShares { .. } => {
                let mut credits = Credits::new();
                for (worker, difficulty) in &self.last {
                    *credits.entry(worker.clone()).or_default() += u128::from(*difficulty);
                }
                credits
            }
        }
    }

    /// Puts a round back when its block did not stay in the chain.
    pub fn reopen(&mut self, credits: Credits) {
        if self.payout == Payout::Proportional {
            for (worker, work) in credits {
                *self.round.entry(worker).or_default() += work;
            }
        }
    }

    /// Work counted toward the next block, by worker.
    pub fn pending(&self) -> Credits {
        match self.payout {
            Payout::Proportional => self.round.clone(),
            Payout::LastShares { .. } => {
                let mut credits = Credits::new();
                for (worker, difficulty) in &self.last {
                    *credits.entry(worker.clone()).or_default() += u128::from(*difficulty);
                }
                credits
            }
        }
    }
}

/// How a block's `value` splits: the pool's fee, the payment's own fee, and each worker's part.
/// The parts, the fee and the payment's fee add up to `value` exactly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Split {
    pub pool_fee: u64,
    pub payment_fee: u64,
    pub parts: Vec<(String, u64)>,
}

/// Splits `value` by `credits` after a fee of `fee_bps` hundredths of a percent and the
/// payment's own fee. Each part is its exact share rounded down; the units rounding leaves go one
/// each to the largest remainders, ties to the first name, so no unit is lost or made up.
pub fn split(value: u64, fee_bps: u32, payment_fee: u64, credits: &Credits) -> Split {
    let fee_bps = u128::from(fee_bps.min(MOST_FEE_BPS));
    let pool_fee = (u128::from(value) * fee_bps / 10_000) as u64;
    // Work past 2^64 is scaled down, so that it times a value of 64 bits still fits.
    let scale = credits.values().fold(0u128, |sum, work| sum.saturating_add(*work)) >> 64;
    let works: Vec<(&String, u128)> = credits.iter().map(|(worker, work)| (worker, work / (scale + 1))).collect();
    let total: u128 = works.iter().map(|(_, work)| work).sum();
    let payment_fee = payment_fee.min(value - pool_fee);
    let shared = u128::from(value - pool_fee - payment_fee);
    if total == 0 {
        // Nobody worked: the pool keeps what it found.
        return Split { pool_fee: pool_fee + shared as u64, payment_fee, parts: Vec::new() };
    }
    let mut parts: Vec<(&String, u64, u128)> = works
        .iter()
        .map(|(worker, work)| {
            let exact = shared * work;
            (*worker, (exact / total) as u64, exact % total)
        })
        .collect();
    let given: u128 = parts.iter().map(|(_, part, _)| u128::from(*part)).sum();
    let mut left = (shared - given) as usize;
    let mut order: Vec<usize> = (0..parts.len()).collect();
    order.sort_by(|a, b| parts[*b].2.cmp(&parts[*a].2).then(parts[*a].0.cmp(parts[*b].0)));
    for at in order {
        if left == 0 {
            break;
        }
        parts[at].1 += 1;
        left -= 1;
    }
    let parts = parts.into_iter().map(|(worker, part, _)| (worker.clone(), part)).collect();
    Split { pool_fee, payment_fee, parts }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn total(split: &Split) -> u128 {
        let parts: u128 = split.parts.iter().map(|(_, part)| u128::from(*part)).sum();
        parts + u128::from(split.pool_fee) + u128::from(split.payment_fee)
    }

    fn credits(pairs: &[(&str, u128)]) -> Credits {
        pairs.iter().map(|(name, work)| (name.to_string(), *work)).collect()
    }

    #[test]
    fn every_unit_of_a_block_lands_somewhere_and_no_more() {
        let three = credits(&[("a", 1), ("b", 1), ("c", 1)]);
        let uneven = credits(&[("a", 7), ("b", u128::MAX / 3), ("c", 1)]);
        for credits in [&three, &uneven] {
            for value in [0, 1, 2, 10, 5_000_000_000, u64::MAX] {
                for fee in [0, 1, 150, 500, 9_999] {
                    for payment_fee in [0, 1, 1_000] {
                        let split = split(value, fee, payment_fee, credits);
                        assert_eq!(total(&split), u128::from(value), "{value} {fee} {payment_fee}");
                    }
                }
            }
        }
        // Ten units over three equal workers: 4, 3, 3, the extra to the first name.
        let ten = split(10, 0, 0, &three);
        assert_eq!(ten.parts, vec![("a".into(), 4), ("b".into(), 3), ("c".into(), 3)]);
    }

    #[test]
    fn a_fee_over_five_percent_is_taken_as_five() {
        let split = split(1_000, 2_000, 0, &credits(&[("a", 7)]));
        assert_eq!(split.pool_fee, 50);
        assert_eq!(split.parts, vec![("a".into(), 950)]);
    }

    #[test]
    fn last_shares_slide_and_a_proportional_round_starts_over() {
        let mut window = Ledger::new(Payout::LastShares { shares: 2 });
        window.add("a", 5);
        window.add("b", 7);
        window.add("b", 9);
        assert_eq!(window.close_round(), credits(&[("b", 16)]));
        // The window does not empty when a block is found: the next block counts them again.
        assert_eq!(window.close_round(), credits(&[("b", 16)]));
        let mut round = Ledger::new(Payout::Proportional);
        round.add("a", 5);
        let closed = round.close_round();
        assert!(round.close_round().is_empty());
        round.reopen(closed);
        assert_eq!(round.close_round(), credits(&[("a", 5)]));
    }
}
