//! One hash budget for this computer. The limit a person sets holds for everything OLC Core mines
//! with here: this app's own miner and every miner added beside it. Without a limit the budget is
//! this computer's highest speed on the mining threads, so a new miner never takes the computer
//! whole. The miners share it equally; one given a speed of its own keeps that speed while it is
//! no more than an equal part, and what it leaves goes to the others, as max-min sharing does.
//! This app's miner always holds its part, so its Start and Stop never move another miner's speed.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Budget {
    /// The person's limit in hashes a second. 0 is none.
    pub limit: u64,
    /// This computer's highest speed on `threads` mining threads.
    pub highest: u64,
    /// The threads every miner here mines on.
    pub threads: usize,
    /// What every miner here may hash together: the limit, or the highest speed without one.
    pub total: u64,
    /// This app's own part.
    pub own: u64,
    /// The part of each added miner that mines now, by its number.
    pub parts: Vec<(u32, u64)>,
    /// What one more miner would start at, as the window says before it is added.
    pub next: u64,
}

impl Budget {
    /// The parts of `total` for this app and for `asked`: each added miner that mines, by its
    /// number, with a speed of its own or 0 for an equal part.
    pub fn share(limit: u64, highest: u64, threads: usize, asked: &[(u32, u64)]) -> Self {
        let total = match limit {
            0 => highest,
            limit if highest > 0 => limit.min(highest),
            limit => limit,
        };
        let wants: Vec<u64> = std::iter::once(0).chain(asked.iter().map(|(_, own)| *own)).collect();
        let given = split(total, &wants);
        let mut with_one_more = wants.clone();
        with_one_more.push(0);
        let next = split(total, &with_one_more).last().copied().unwrap_or(0);
        Self {
            limit,
            highest,
            threads,
            total,
            own: given[0],
            parts: asked.iter().map(|(id, _)| *id).zip(given[1..].iter().copied()).collect(),
            next,
        }
    }

    /// The part of the added miner `id`, while it mines.
    pub fn part_of(&self, id: u32) -> Option<u64> {
        self.parts.iter().find(|(known, _)| *known == id).map(|(_, part)| *part)
    }

    /// True when this app mines alone with no limit: it then runs uncapped, as it always did.
    pub fn unbounded(&self) -> bool {
        self.limit == 0 && self.parts.is_empty()
    }
}

/// Max-min sharing of `total` among `wants`, where 0 asks for an equal part. A speed of its own
/// below an equal part is given whole and the rest is shared again among the others; whatever a
/// division leaves over goes to the first, this app. The parts never add to more than `total`.
fn split(total: u64, wants: &[u64]) -> Vec<u64> {
    let mut given = vec![0; wants.len()];
    let mut open: Vec<usize> = (0..wants.len()).collect();
    let mut left = total;
    loop {
        let Some(equal) = left.checked_div(open.len() as u64) else {
            return given;
        };
        let small: Vec<usize> = open.iter().copied().filter(|at| wants[*at] > 0 && wants[*at] <= equal).collect();
        if small.is_empty() {
            for at in &open {
                given[*at] = equal;
            }
            if let Some(first) = open.first() {
                given[*first] += left - equal * open.len() as u64;
            }
            return given;
        }
        for at in small {
            given[at] = wants[at];
            left -= wants[at];
            open.retain(|other| *other != at);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_miner_shares_the_limit_with_this_app() {
        let alone = Budget::share(2_000, 7_000_000, 1, &[]);
        assert_eq!((alone.total, alone.own, alone.next), (2_000, 2_000, 1_000));
        let two = Budget::share(2_000, 7_000_000, 1, &[(4, 0)]);
        assert_eq!((two.own, two.part_of(4)), (1_000, Some(1_000)));
        let three = Budget::share(2_000, 7_000_000, 1, &[(4, 0), (5, 0)]);
        assert_eq!(three.own + three.parts.iter().map(|(_, part)| part).sum::<u64>(), 2_000, "{three:?}");
        assert_eq!(three.part_of(5), Some(666));
    }

    #[test]
    fn without_a_limit_the_computer_is_shared_never_taken_whole() {
        let budget = Budget::share(0, 8_000_000, 11, &[]);
        assert!(budget.unbounded());
        assert_eq!(budget.next, 4_000_000, "a new miner starts at half, beside this app");
        let shared = Budget::share(0, 8_000_000, 11, &[(0, 0)]);
        assert!(!shared.unbounded());
        assert_eq!((shared.own, shared.part_of(0)), (4_000_000, Some(4_000_000)));
    }

    #[test]
    fn a_speed_of_its_own_is_kept_inside_the_limit() {
        // A board's preset asks 15,000: under a limit of 100,000 it keeps it, under 2,000 it gets
        // an equal part and the limit holds.
        let roomy = Budget::share(100_000, 7_000_000, 1, &[(1, 15_000), (2, 0)]);
        assert_eq!((roomy.part_of(1), roomy.part_of(2), roomy.own), (Some(15_000), Some(42_500), 42_500));
        let tight = Budget::share(2_000, 7_000_000, 1, &[(1, 15_000)]);
        assert_eq!((tight.part_of(1), tight.own), (Some(1_000), 1_000));
        let small = Budget::share(2_000, 7_000_000, 1, &[(1, 300), (2, 0)]);
        assert_eq!((small.part_of(1), small.part_of(2), small.own), (Some(300), Some(850), 850));
    }

    #[test]
    fn a_limit_above_the_computer_is_the_computer() {
        assert_eq!(Budget::share(50_000_000, 7_000_000, 1, &[]).total, 7_000_000);
        assert_eq!(Budget::share(5_000, 0, 1, &[]).total, 5_000, "before the speed is measured");
    }
}
