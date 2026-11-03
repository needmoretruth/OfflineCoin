//! A network split and its end. Two groups of miners agree on one chain, the link between them
//! is cut, both keep mining their own branch, and when they are joined again every miner takes
//! the branch with more work. The other branch's blocks are replaced. This is what happens to
//! any proof-of-work coin when a network falls apart for a while.
//! The right group is one miner, cut off with `offline on`, so the left group keeps hearing
//! itself.
use crate::steps::{Step, Watch};
use olc_board::{Board, BoardLink, Say, Status};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_millis(500);
const SETTLE_WAIT: Duration = Duration::from_secs(120);

pub struct SplitOptions {
    /// Blocks each branch mines on its own before the groups join again.
    pub apart_blocks: u64,
    /// The groups join again after this long even if a branch is short.
    pub apart_limit: Duration,
    pub timeout: Duration,
}

struct Split<'a> {
    left: Vec<Board>,
    right: Board,
    say: &'a Say,
    watch: &'a Watch,
    stop: &'a AtomicBool,
    deadline: Instant,
}

/// Runs on the left group's links and the one right miner.
pub fn split_with(
    left: Vec<Box<dyn BoardLink>>,
    right: Box<dyn BoardLink>,
    options: SplitOptions,
    say: Say,
    watch: Watch,
    stop: Arc<AtomicBool>,
) -> Result<(), String> {
    if left.is_empty() {
        return Err("The left group needs a miner.".into());
    }
    let mut split = Split {
        left: left.into_iter().map(|link| Board::new("left", link)).collect(),
        right: Board::new("right", right),
        say: &say,
        watch: &watch,
        stop: &stop,
        deadline: Instant::now() + options.timeout,
    };
    let result = split.run(&options);
    // Whatever happened, the right miner hears the others again.
    let back = split.right.set_offline(false);
    if let Err(error) = &result {
        say(format!("Stopped: {error}"));
    }
    result.and(back)
}

impl Split<'_> {
    fn pause(&self) -> Result<(), String> {
        if self.stop.load(Ordering::Relaxed) {
            return Err("stopped by hand.".into());
        }
        if Instant::now() > self.deadline {
            return Err("out of time.".into());
        }
        thread::sleep(POLL);
        Ok(())
    }

    fn lefts(&mut self) -> Result<Vec<Status>, String> {
        self.left.iter_mut().map(Board::status).collect()
    }

    fn run(&mut self, options: &SplitOptions) -> Result<(), String> {
        (self.say)("\n1. Both groups mine one chain".to_string());
        for board in &mut self.left {
            board.set_offline(false)?;
            board.set_mining(true)?;
        }
        self.right.set_offline(false)?;
        self.right.set_mining(true)?;
        let (left, right) = self.power()?;
        (self.watch)(Step::Power { attacker: left, victim: right });
        let height = self.agree()?;
        (self.say)(format!("   every miner agrees at height {height}"));
        (self.watch)(Step::LinedUp { height });

        (self.say)("\n2. The link between the groups is cut".to_string());
        self.right.set_offline(true)?;
        let cut_at = self.right.status()?.height;
        (self.watch)(Step::Cut { height: cut_at });
        let started = Instant::now();
        loop {
            let left = self.lefts()?.into_iter().map(|status| status.height).max().unwrap_or(0);
            let right = self.right.status()?.height;
            let grown = |height: u64| height.saturating_sub(cut_at) >= options.apart_blocks;
            if (grown(left) && grown(right)) || started.elapsed() > options.apart_limit {
                (self.say)(format!("   left branch at height {left}, right branch at height {right}"));
                break;
            }
            self.pause()?;
        }

        (self.say)("\n3. The groups are joined again".to_string());
        let before = self.right.status()?.reorg;
        let before_left: Vec<(u64, u64)> = self.lefts()?.iter().map(|status| status.reorg).collect();
        self.right.set_offline(false)?;
        (self.watch)(Step::Rejoined);
        let height = self.agree()?;
        // A miner counts a branch move just after it makes it, so look once more.
        self.pause()?;
        let right = self.right.status()?;
        let lefts = self.lefts()?;
        let left_moved = lefts.iter().zip(&before_left).find(|(now, then)| now.reorg != **then);
        let (left_won, (at, blocks)) = match (right.reorg != before, left_moved) {
            (true, _) => (true, right.reorg),
            (false, Some((now, _))) => (false, now.reorg),
            (false, None) => return Err("no miner changed branch.".into()),
        };
        let winner = if left_won { "left" } else { "right" };
        (self.say)(format!(
            "   the {winner} branch had more work: {blocks} blocks from height {at} were replaced"
        ));
        (self.watch)(Step::Settled { left_won, height, blocks });
        Ok(())
    }

    /// Hashes a second on each side, over a few looks.
    fn power(&mut self) -> Result<(u64, u64), String> {
        let (mut left, mut right) = (0, 0);
        const LOOKS: u64 = 4;
        for _ in 0..LOOKS {
            self.pause()?;
            left += self.lefts()?.iter().map(|status| status.hashrate).sum::<u64>();
            right += self.right.status()?.hashrate;
        }
        Ok((left / LOOKS, right / LOOKS))
    }

    /// Waits until every miner ends in the same block.
    fn agree(&mut self) -> Result<u64, String> {
        let until = Instant::now() + SETTLE_WAIT;
        loop {
            let right = self.right.status()?;
            let lefts = self.lefts()?;
            if lefts.iter().all(|status| status.tip == right.tip) {
                return Ok(right.height);
            }
            if Instant::now() > until {
                return Err("the miners did not agree on one chain.".into());
            }
            self.pause()?;
        }
    }
}
