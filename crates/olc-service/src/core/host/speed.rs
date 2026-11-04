//! The hash cap's one second windows, and the speed a miner shows from them: the hashes granted
//! over the last five seconds, as cgminer shows its 5 s average. A window never grants more than
//! the cap allows for the time it has run, and the windows follow one another without a gap, so
//! the speed over any run of them is never above the cap. A reading over the hashes a thread
//! happened to finish in one second, the way it was read before, could land a tenth above it.
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// How far back the speed reads.
pub const SPEED_SPAN: Duration = Duration::from_secs(5);
const WINDOW_MS: u64 = 1_000;

pub(super) struct Windows {
    /// The windows closed within the span, oldest first: when each began and what it granted.
    past: VecDeque<(Instant, u64)>,
    /// The open window: when it began and what it granted so far.
    start: Instant,
    granted: u64,
    /// The cap the windows were granted under; another starts them over.
    cap: u64,
}

impl Windows {
    pub(super) fn new(now: Instant) -> Self {
        Self { past: VecDeque::new(), start: now, granted: 0, cap: 0 }
    }

    /// Forgets what was granted, as when the crew rests or the cap changes: a speed read across
    /// an older cap could stand above the new one.
    pub(super) fn restart(&mut self, now: Instant) {
        self.past.clear();
        (self.start, self.granted) = (now, 0);
    }

    fn millis(from: Instant, to: Instant) -> u64 {
        u64::try_from(to.saturating_duration_since(from).as_millis()).unwrap_or(u64::MAX)
    }

    /// Hashes a thread may try now, `batch` at most, under `cap` a second for every thread
    /// together; 0 is no cap. What is granted is counted for the speed.
    pub(super) fn grant(&mut self, cap: u64, batch: u32, now: Instant) -> u32 {
        if cap != self.cap {
            self.cap = cap;
            self.restart(now);
        }
        let mut elapsed = Self::millis(self.start, now);
        if elapsed >= WINDOW_MS {
            self.past.push_back((self.start, self.granted));
            while self.past.front().is_some_and(|(start, _)| now.saturating_duration_since(*start) > SPEED_SPAN + Duration::from_millis(WINDOW_MS)) {
                self.past.pop_front();
            }
            (self.start, self.granted, elapsed) = (now, 0, 0);
        }
        if cap == 0 {
            self.granted += u64::from(batch);
            return batch;
        }
        let allowed = cap.saturating_mul(elapsed) / 1_000;
        // A crew held up by a block or a busy machine does not race to make up the whole lag at the
        // end of the second. At most a twentieth of a second's hashes wait to be made up.
        let most_behind = (cap / 20).max(u64::from(batch));
        if allowed.saturating_sub(self.granted) > most_behind {
            self.granted = allowed - most_behind;
        }
        // A batch goes only once the time has earned all of it, so a window never holds more than
        // the cap allows for its time.
        if self.granted + u64::from(batch) > allowed {
            return 0;
        }
        self.granted += u64::from(batch);
        batch
    }

    /// Hashes a second over the windows that began within the last [`SPEED_SPAN`], up to `now`.
    /// Under a cap it is never above it. Without one a window just begun reads over a whole second,
    /// so a first batch does not read as a burst.
    pub(super) fn speed(&self, now: Instant) -> u64 {
        let within = |start: &Instant| now.saturating_duration_since(*start) <= SPEED_SPAN;
        let from = self.past.iter().map(|(start, _)| *start).find(within).unwrap_or(self.start);
        let past: u64 = self.past.iter().filter(|(start, _)| *start >= from).map(|(_, granted)| granted).sum();
        let mut span = Self::millis(from, now);
        if self.cap == 0 {
            span = span.max(WINDOW_MS);
        }
        match span {
            0 => 0,
            span => (past + self.granted).saturating_mul(1_000) / span,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A crew of four asks for hashes every millisecond and now and then stalls, as a busy computer
    /// stalls it: read every quarter second over a minute, the speed is never above the cap, and
    /// its mean is the cap within a few percent.
    #[test]
    fn the_speed_shown_never_stands_above_the_cap() {
        let cap = 59_868;
        let start = Instant::now();
        let mut windows = Windows::new(start);
        let mut readings = Vec::new();
        for ms in 0..60_000u64 {
            // A stall of 80 ms every 1.3 s, a block being built on a busy machine.
            if ms % 1_300 < 80 {
                continue;
            }
            let now = start + Duration::from_millis(ms);
            for _ in 0..4 {
                windows.grant(cap, (cap / 100) as u32, now);
            }
            if ms % 250 == 0 && ms >= 1_000 {
                readings.push(windows.speed(now));
            }
        }
        let most = readings.iter().copied().max().unwrap();
        let mean = readings.iter().sum::<u64>() / readings.len() as u64;
        assert!(most <= cap, "a reading of {most} above the cap {cap}");
        assert!(mean * 100 >= cap * 90, "the mean {mean} keeps near the cap {cap}");
    }

    /// A lower cap reads under itself from the moment it is set, not five seconds later; resting
    /// reads 0 at once; with no cap the first batch is not a burst.
    #[test]
    fn a_new_cap_and_a_rest_read_at_once() {
        let start = Instant::now();
        let mut windows = Windows::new(start);
        for ms in 0..5_000u64 {
            windows.grant(100_000, 1_000, start + Duration::from_millis(ms));
        }
        let at = |ms: u64| start + Duration::from_millis(ms);
        assert!(windows.speed(at(5_000)) > 90_000);
        for ms in 5_000..5_300u64 {
            windows.grant(20_000, 200, at(ms));
            assert!(windows.speed(at(ms)) <= 20_000, "{} at {ms}", windows.speed(at(ms)));
        }
        windows.restart(at(5_300));
        assert_eq!(windows.speed(at(5_301)), 0);
        let mut open = Windows::new(start);
        open.grant(0, 5_000, start);
        assert_eq!(open.speed(start + Duration::from_millis(1)), 5_000);
    }
}
