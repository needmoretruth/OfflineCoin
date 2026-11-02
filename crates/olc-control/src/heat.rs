//! The heat guard. A board left alone for years slows itself when its chip gets hot, whatever
//! speed or cap it was told, and goes back to them once it has really cooled.
//!
//! The N16R8 module is rated for 65 C in the air around it, and the chip inside always reads above
//! that air. Nothing here reads the chip's factory offset, so 5 C of the room goes to the sensor:
//! a reading of 60 means the air is below the rating even if the sensor reads low by that much.
//! Measured, the hottest a board gets in a room is 49 C at 240 MHz with no cap, so the guard does
//! not stand in the way of a person who chose full speed. It only acts in a hot place.

/// The chip reading where the board slows.
pub const HOT_C: f32 = 60.0;
/// The chip reading where it speeds up again. Ten degrees apart, so it does not flip back and forth.
pub const COOL_C: f32 = 50.0;
/// Hashes per second while it cools: mining goes on, and the chip stays close to idle.
pub const COOLING_CAP: u32 = 100;
/// Readings in a row before anything changes, so one bad reading changes nothing.
const READS_TO_CHANGE: u8 = 3;
/// The range of the sensor. Anything else is not a reading.
const SENSOR_RANGE: core::ops::RangeInclusive<f32> = -40.0..=125.0;

pub struct HeatGuard {
    hot: f32,
    cool: f32,
    cooling: bool,
    in_a_row: u8,
}

impl Default for HeatGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl HeatGuard {
    pub const fn new() -> Self {
        Self { hot: HOT_C, cool: COOL_C, cooling: false, in_a_row: 0 }
    }

    /// Takes one reading. A sensor that gave none leaves things as they are.
    pub fn read(&mut self, celsius: Option<f32>) {
        let Some(celsius) = celsius.filter(|value| SENSOR_RANGE.contains(value)) else {
            self.in_a_row = 0;
            return;
        };
        let wants_change = match self.cooling {
            true => celsius <= self.cool,
            false => celsius >= self.hot,
        };
        self.in_a_row = if wants_change { self.in_a_row + 1 } else { 0 };
        if self.in_a_row >= READS_TO_CHANGE {
            self.cooling = !self.cooling;
            self.in_a_row = 0;
        }
    }

    pub fn cooling(&self) -> bool {
        self.cooling
    }

    /// The hashes per second to run at, from the cap the person chose, where 0 means no cap. Their
    /// choice is not touched: it is back as soon as the board has cooled.
    pub fn cap(&self, chosen: u32) -> u32 {
        match (self.cooling, chosen) {
            (false, _) => chosen,
            (true, 0) => COOLING_CAP,
            (true, chosen) => chosen.min(COOLING_CAP),
        }
    }

    /// Lower limits until the next start, to prove the guard on a real board without cooking it.
    /// It can only be made stricter. False when the numbers would not do.
    pub fn try_limits(&mut self, hot: f32, cool: f32) -> bool {
        let fits = hot <= HOT_C && cool < hot;
        if fits {
            (self.hot, self.cool) = (hot, cool);
        }
        fits
    }

    pub fn usual_limits(&mut self) {
        (self.hot, self.cool) = (HOT_C, COOL_C);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fed(guard: &mut HeatGuard, celsius: f32, reads: u8) {
        for _ in 0..reads {
            guard.read(Some(celsius));
        }
    }

    #[test]
    fn a_hot_chip_slows_whatever_was_chosen_and_keeps_the_choice() {
        let mut guard = HeatGuard::new();
        fed(&mut guard, 59.9, 50);
        assert!(!guard.cooling());
        assert_eq!(guard.cap(0), 0);
        fed(&mut guard, HOT_C, 3);
        assert!(guard.cooling());
        assert_eq!(guard.cap(0), COOLING_CAP, "no cap is the case that needs it most");
        assert_eq!(guard.cap(1_000), COOLING_CAP);
        assert_eq!(guard.cap(30), 30, "a lower cap the person chose stands");
        fed(&mut guard, 40.0, 3);
        assert_eq!(guard.cap(0), 0);
        assert_eq!(guard.cap(1_000), 1_000);
    }

    #[test]
    fn it_comes_back_only_after_it_has_really_cooled() {
        let mut guard = HeatGuard::new();
        fed(&mut guard, 61.0, 3);
        fed(&mut guard, 59.0, 100);
        fed(&mut guard, 50.1, 100);
        assert!(guard.cooling(), "just under the limit is not cool");
        fed(&mut guard, COOL_C, 3);
        assert!(!guard.cooling());
    }

    #[test]
    fn one_bad_reading_changes_nothing() {
        let mut guard = HeatGuard::new();
        for _ in 0..20 {
            guard.read(Some(91.0));
            guard.read(Some(45.0));
        }
        assert!(!guard.cooling());
        guard.read(Some(70.0));
        guard.read(None);
        fed(&mut guard, 70.0, 2);
        assert!(!guard.cooling(), "a missing reading breaks the run");
        fed(&mut guard, f32::NAN, 9);
        fed(&mut guard, 400.0, 9);
        assert!(!guard.cooling(), "these are not readings");
        fed(&mut guard, 70.0, 3);
        fed(&mut guard, f32::NAN, 9);
        assert!(guard.cooling(), "a sensor gone quiet does not count as cool");
    }

    #[test]
    fn a_test_can_only_make_it_stricter() {
        let mut guard = HeatGuard::new();
        assert!(!guard.try_limits(61.0, 50.0));
        assert!(!guard.try_limits(40.0, 40.0));
        assert!(guard.try_limits(40.0, 35.0));
        fed(&mut guard, 41.0, 3);
        assert!(guard.cooling());
        fed(&mut guard, 35.0, 3);
        assert!(!guard.cooling());
        guard.usual_limits();
        fed(&mut guard, 41.0, 3);
        assert!(!guard.cooling());
    }
}
