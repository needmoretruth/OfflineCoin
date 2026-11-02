//! What keeps a board whole over years with nobody watching: its heat, its panel, and a restart
//! if the program ever stops.
use crate::node::{device_ms, Node};
use crate::thermo;
use alloc::format;
use esp_hal::rtc_cntl::{Rwdt, RwdtStage};
use esp_hal::time::Duration;
use olc_control::heat::HeatGuard;
use olc_control::panel;

/// A new build that stops within this much running goes back to the build before. It covers what
/// starts after the main loop, such as joining a router.
const GOOD_AFTER_MS: u64 = 60_000;
/// Longer than anything a turn can take. A program that stopped would otherwise leave the last
/// picture lit on the panel for as long as the power stays on.
const RUN_WATCHDOG_S: u64 = 300;

pub struct Care {
    pub heat: HeatGuard,
    watchdog: Rwdt,
    touched_ms: u64,
    shown_bright: u8,
    /// The longest turn of the main loop since `status` last asked.
    longest_turn_us: u64,
    marked_good: bool,
}

impl Care {
    pub fn new(mut watchdog: Rwdt, start_bright: u8) -> Self {
        watchdog.set_timeout(RwdtStage::Stage0, Duration::from_secs(RUN_WATCHDOG_S));
        watchdog.enable();
        Self {
            heat: HeatGuard::new(),
            watchdog,
            touched_ms: device_ms(),
            shown_bright: start_bright,
            longest_turn_us: 0,
            marked_good: false,
        }
    }

    pub fn touched(&mut self, now: u64) {
        self.touched_ms = now;
    }

    pub fn shown_bright(&self) -> u8 {
        self.shown_bright
    }

    pub fn timed_turn(&mut self, length_us: u64) {
        self.longest_turn_us = self.longest_turn_us.max(length_us);
    }

    pub fn take_longest_turn(&mut self) -> u64 {
        core::mem::take(&mut self.longest_turn_us)
    }
}

impl Node {
    /// Once a second, from the main loop.
    pub fn look_after(&mut self, now: u64) {
        self.care.watchdog.feed();
        if !self.care.marked_good && now >= GOOD_AFTER_MS {
            self.care.marked_good = true;
            self.store.mark_running_build_good();
        }
        self.care.heat.read(thermo::read());
        let quiet = now.saturating_sub(self.care.touched_ms);
        let shown = panel::shown_bright(self.bright, quiet);
        if shown != self.care.shown_bright {
            self.care.shown_bright = shown;
            self.screen.set_bright(shown);
        }
    }

    /// `heat HOT COOL` makes the guard stricter until the next start, and `heat default` ends that.
    pub fn set_heat(&mut self, hot: &str, cool: &str) {
        let parsed = hot.parse().ok().zip(cool.parse().ok());
        let set = parsed.is_some_and(|(hot, cool)| self.care.heat.try_limits(hot, cool));
        self.say(if set { format!("heat {hot} {cool}") } else { "error heat HOT COOL".into() });
    }
}
