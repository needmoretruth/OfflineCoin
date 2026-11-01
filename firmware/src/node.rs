//! One pass of the board's work: move messages, mine a little, save, redraw.
use crate::clock;
use crate::control::{cable_is_busy, parse_key, Control, BUILD};
use crate::radio;
use crate::screen::{Screen, PAGES, WIDTH};
use crate::store::{Settings, Store};
use crate::usb::Commands;
use crate::views::{self, Coins, Info, Language, Note, State, View};
use crate::wifi::Network;
use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::String;
use core::sync::atomic::Ordering;
use esp_hal::gpio::Input;
use esp_hal::time::Instant;
use esp_println::println;
use olc_control::bootlog;
use olc_core::coin::COIN;
use olc_core::Hash;
use olc_logo::{MAX_FPS, MIN_FPS};
use olc_node::{Event, Runner};
use olc_wallet::{format_address, parse_address};

/// The order links are added in, which is how peers are counted per link.
pub const LINK_RADIO: usize = 0;
pub const LINK_USB: usize = 1;
pub const LINK_WIFI: usize = 2;

/// About 13 ms of work, short enough to answer the radio between batches.
const MINING_BATCH: u32 = 200;
const STATUS_EVERY_MS: u64 = 10_000;
const MAX_DROPPED: usize = 8;
const MAX_NOTES: usize = 5;
const BLOCKS_AVERAGED: u64 = 10;
pub const SPLASH_LENGTH_MS: u64 = 2_500;
pub use olc_control::defaults::BOARD_CAP as RELAXED_LIMIT;
/// How often words and numbers change. The coin has its own pace.
const TEXT_EVERY_MS: u64 = 250;
/// A short mining batch while the coin shows, so a frame never waits long.
const COIN_MINING_BATCH: u32 = 60;
/// Coin rows drawn on each pass of the loop, about 0.3 ms each.
const COIN_ROWS_PER_TURN: usize = 2;
/// The coin's frames a second on a board whose owner never chose.
pub use olc_control::defaults::BOARD_SCREEN_RATE as DEFAULT_SMOOTH;
const RESTART_DELAY_MS: u64 = 300;
/// Radio power on a board whose owner never chose, enough for one desk or one room.
pub const DEFAULT_POWER_DBM: i8 = 5;
const POWER_RANGE: core::ops::RangeInclusive<i8> = 2..=20;
const BOOTS_SHOWN: usize = 8;
const BUTTON_DEBOUNCE_MS: u64 = 30;
const BUTTON_HOLD_MS: u64 = 800;
/// Mining stops here, so a full memory ends in a message and not a crash.
const MIN_FREE_HEAP: usize = 256 * 1024;
/// A line on the cable counts as finished this long after its last byte.
pub const CABLE_QUIET_MS: u32 = 20;

/// How the coin really ran, for the status line.
#[derive(Default)]
pub struct FrameCount {
    pub sent: u32,
    pub slowest_us: u64,
    pub last_second: u32,
    pub last_slowest_us: u64,
}

impl FrameCount {
    fn roll(&mut self) {
        self.last_second = core::mem::take(&mut self.sent);
        self.last_slowest_us = core::mem::take(&mut self.slowest_us);
    }
}

pub fn device_ms() -> u64 {
    Instant::now().duration_since_epoch().as_millis()
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn decode_hash(text: &str, out: &mut Hash) -> bool {
    if text.len() != out.len() * 2 || !text.is_ascii() {
        return false;
    }
    out.iter_mut().enumerate().all(|(index, byte)| {
        match u8::from_str_radix(&text[index * 2..index * 2 + 2], 16) {
            Ok(value) => {
                *byte = value;
                true
            }
            Err(_) => false,
        }
    })
}

/// How this start went, kept for `status`.
pub struct Started {
    pub reset: u8,
    pub panel_ms: u64,
    pub read_ms: u64,
    pub placed_ms: u64,
    pub loaded_ms: u64,
}

pub struct Node {
    pub runner: Runner,
    pub control: Control,
    pub store: Store,
    pub screen: Screen,
    pub commands: Commands,
    pub network: Network,
    pub coins: Coins,
    /// Frames a second of the coin.
    pub smooth: u32,
    pub coin_ms: u64,
    /// Coin frames sent and the slowest send, over the last second and the one before.
    pub frames: FrameCount,
    pub power_dbm: i8,
    /// The panel's contrast.
    pub bright: u8,
    /// Whether the watchdog was running when this program got the chip.
    pub boot_watchdog: bool,
    pub clock_mhz: u32,
    /// When to restart, so that the answer to the order leaves first.
    pub restart_ms: Option<u64>,
    /// Time spent working in this second, and how much of the last second that was.
    pub busy_us: u64,
    pub busy_percent: u64,
    /// The BOOT button: a press changes the view, a long press changes the language.
    pub button: Input<'static>,
    pub view: View,
    pub language: Language,
    pub mining: bool,
    /// Hashes per second, 0 for full. Saved once chosen, so a cap outlives a restart.
    pub limit: u32,
    pub limit_chosen: bool,
    pub started: Started,
    pub memory_full: bool,
    pub attempts: u32,
    pub rate: u32,
    /// The last branch change as (height, blocks replaced), kept because a line can get lost.
    pub last_reorg: (u64, u64),
    /// The payments thrown out last, with why.
    pub dropped: VecDeque<(Hash, String)>,
    pub window_ms: u64,
    /// Attempts since the tip last changed, which is what the bar on the screen shows.
    pub tip_attempts: u64,
    pub tip_seen: Hash,
    pub mined: u32,
    /// The tip's height this board last saw change, for the age of the last block.
    pub tip_seen_ms: u64,
    pub block_time_ms: Option<u64>,
    pub event: Option<(u64, Note)>,
    /// The latest things that happened, oldest first.
    pub notes: VecDeque<Note>,
    pub screen_ms: u64,
    pub status_ms: u64,
    pub pressed_ms: Option<u64>,
    /// The spinning logo shows until this time, which is on every start.
    pub splash_until_ms: u64,
}

impl Node {
    /// Returns true when it did mining work, false when the caller may sleep.
    pub fn turn(&mut self) -> bool {
        if self.restart_ms.is_some_and(|at| device_ms() >= at) {
            esp_hal::system::software_reset();
        }
        let now = device_ms();
        self.runner.step(now);
        self.serve_control(now);
        while let Some(line) = self.next_command() {
            self.run_command(&line);
        }
        self.read_button(now);
        let window = now.saturating_sub(self.window_ms);
        if window >= 1_000 {
            self.rate = (u64::from(self.attempts) * 1_000 / window) as u32;
            self.busy_percent = self.busy_us / 10 / window;
            self.busy_us = 0;
            self.attempts = 0;
            self.window_ms = now;
            self.frames.roll();
        }
        if esp_alloc::HEAP.free() < MIN_FREE_HEAP {
            self.mining = false;
            self.memory_full = true;
        }
        let longest = if self.coin_shows(now) {
            COIN_MINING_BATCH
        } else {
            MINING_BATCH
        };
        let batch = match self.limit {
            0 => longest,
            limit => longest.min(limit.saturating_sub(self.attempts)),
        };
        let worked =
            self.mining && batch > 0 && !cable_is_busy(now) && !self.control.taking_an_image(now);
        if worked {
            self.runner.engine.mine(batch, now);
            self.attempts += batch;
            self.tip_attempts += u64::from(batch);
        }
        self.handle_events(now);
        while let Some(block) = self.runner.engine.take_fresh_block() {
            self.store.append(&block);
        }
        let tip = self.runner.engine.chain().tip();
        if tip != self.tip_seen {
            self.tip_seen = tip;
            self.tip_seen_ms = now;
            self.tip_attempts = 0;
            self.block_time_ms = self.recent_block_time();
        }
        self.coins.work(COIN_ROWS_PER_TURN);
        let started = Instant::now();
        let frame_due = now >= self.coin_ms && self.coin_shows(now);
        if now >= self.screen_ms {
            self.draw(now);
        } else if frame_due {
            self.coins
                .next_frame(&mut self.screen, now, now < self.splash_until_ms);
        }
        if frame_due {
            self.count_coin_frame(now, started);
        }
        if now >= self.status_ms {
            self.status_ms = now + STATUS_EVERY_MS;
            self.print_status();
        }
        worked
    }

    fn coin_shows(&self, now: u64) -> bool {
        now < self.splash_until_ms || self.view == View::Easy
    }

    /// Keeps to the pace rather than restarting it, so waiting a moment costs no frame.
    fn count_coin_frame(&mut self, now: u64, started: Instant) {
        self.coin_ms = (self.coin_ms + u64::from(1_000 / self.smooth)).max(now);
        self.frames.sent += 1;
        self.frames.slowest_us = self.frames.slowest_us.max(started.elapsed().as_micros());
    }

    fn next_command(&self) -> Option<String> {
        self.commands.borrow_mut().pop_front()
    }

    fn read_button(&mut self, now: u64) {
        match (self.button.is_low(), self.pressed_ms) {
            (true, None) => self.pressed_ms = Some(now),
            (false, Some(pressed)) => {
                self.pressed_ms = None;
                let held = now.saturating_sub(pressed);
                if held >= BUTTON_HOLD_MS {
                    self.set_language(match self.language {
                        Language::English => Language::Korean,
                        Language::Korean => Language::English,
                    });
                } else if held >= BUTTON_DEBOUNCE_MS {
                    self.set_view(self.view.next());
                }
            }
            _ => {}
        }
    }

    fn set_view(&mut self, view: View) {
        self.view = view;
        self.screen_ms = 0;
        self.save(|settings| settings.view = view.name().into());
    }

    fn set_language(&mut self, language: Language) {
        self.language = language;
        self.screen_ms = 0;
        self.save(|settings| settings.language = language.name().into());
    }

    /// `None` goes back to what a board does when nobody chose.
    fn set_limit(&mut self, choice: Option<u32>) {
        let limit = choice.unwrap_or(RELAXED_LIMIT);
        if limit != self.limit || choice.is_some() != self.limit_chosen {
            self.limit = limit;
            self.limit_chosen = choice.is_some();
            self.save(|settings| {
                settings.limit.clear();
                settings.cap = choice.map(|limit| format!("{limit}")).unwrap_or_default();
            });
        }
    }

    fn set_smooth(&mut self, fps: u32) {
        if fps != self.smooth {
            self.smooth = fps;
            self.save(|settings| settings.smooth = format!("{fps}"));
        }
    }

    fn save(&mut self, change: impl FnOnce(&mut Settings)) {
        let mut settings = self.store.settings();
        change(&mut settings);
        self.store.save_settings(&settings);
    }

    fn handle_events(&mut self, now: u64) {
        while let Some(event) = self.runner.engine.take_event() {
            match event {
                Event::Mined { height } => {
                    self.mined += 1;
                    self.say(format!("mined {height}"));
                }
                Event::BlockAccepted { height } => self.say(format!("block {height}")),
                Event::Reorganized { replaced, height } => {
                    self.last_reorg = (height, replaced);
                    self.say(format!("reorg {height} replaced={replaced}"));
                }
                Event::BlockRejected { reason } => self.say(format!("rejected block {reason:?}")),
                Event::PaymentReceived { amount } => self.say(format!("received {amount}")),
                Event::PaymentSent { amount } => self.say(format!("sent {amount}")),
                Event::TxAccepted => self.say("payment accepted".into()),
                Event::TxRejected { reason } => self.say(format!("rejected payment {reason:?}")),
                Event::TxReturned { id } => self.say(format!("returned {}", hex(&id))),
                Event::TxDropped { id, reason } => {
                    self.say(format!("dropped {} {reason:?}", hex(&id)));
                    if self.dropped.len() >= MAX_DROPPED {
                        self.dropped.pop_front();
                    }
                    self.dropped.push_back((id, format!("{reason:?}")));
                }
            }
            self.note(now, Note::Chain(event));
        }
    }

    /// Puts something on the panel for a moment, and on the page of events when it matters.
    pub fn note(&mut self, now: u64, note: Note) {
        self.event = Some((now, note));
        if !matches!(note, Note::Chain(Event::BlockAccepted { .. })) {
            if self.notes.len() >= MAX_NOTES {
                self.notes.pop_front();
            }
            self.notes.push_back(note);
        }
    }

    fn recent_block_time(&self) -> Option<u64> {
        let chain = self.runner.engine.chain();
        let newest = chain.tip_header().time_ms;
        let mut cursor = chain.tip_header().previous;
        let mut oldest = newest;
        let mut blocks = 0;
        while blocks < BLOCKS_AVERAGED {
            let Some(block) = chain.block(&cursor) else {
                break;
            };
            oldest = block.header.time_ms;
            cursor = block.header.previous;
            blocks += 1;
        }
        (blocks > 0).then(|| newest.saturating_sub(oldest) / blocks)
    }

    pub fn run_command(&mut self, line: &str) {
        let mut words = line.split_whitespace();
        match (words.next(), words.next(), words.next()) {
            (Some("status"), _, _) => self.print_status(),
            (Some("screen"), _, _) => self.print_screen(),
            (Some("boots"), _, _) => {
                let words = self.store.boot_words(BOOTS_SHOWN);
                let recent = bootlog::recent(&words, BOOTS_SHOWN);
                self.say(format!("boots {}", recent.join(" ")));
            }
            (Some("logo"), _, _) => self.splash_until_ms = device_ms() + SPLASH_LENGTH_MS,
            (Some("view"), Some(name), _) => {
                if let Some(view) = View::from_name(name) {
                    self.set_view(view);
                }
            }
            (Some("lang"), Some(name), _) => {
                if let Some(language) = Language::from_name(name) {
                    self.set_language(language);
                }
            }
            (Some("mine"), Some(state), _) => self.mining = state == "on",
            (Some("limit"), Some("default"), _) => self.set_limit(None),
            (Some("limit"), Some(value), _) => {
                if let Ok(limit) = value.parse() {
                    self.set_limit(Some(limit));
                }
            }
            (Some("smooth"), Some(value), _) => match value.parse() {
                Ok(fps) if (MIN_FPS..=MAX_FPS).contains(&fps) => self.set_smooth(fps),
                _ => self.say(format!("error smooth {MIN_FPS} to {MAX_FPS}")),
            },
            (Some("power"), Some(value), _) => match value.parse() {
                Ok(dbm) if POWER_RANGE.contains(&dbm) => {
                    self.restart_with(|settings| settings.power = format!("{dbm}"));
                }
                _ => self.say("error power 2 to 20".into()),
            },
            (Some("bright"), Some(value), _) => match value.parse() {
                Ok(bright) => {
                    self.screen.set_bright(bright);
                    self.bright = bright;
                    self.save(|settings| settings.bright = format!("{bright}"));
                }
                _ => self.say("error bright 0 to 255".into()),
            },
            (Some("ignore"), Some("none"), _) => radio::hear_everyone(),
            (Some("ignore"), Some(peer), _) => {
                match u32::from_str_radix(peer, 16).map(radio::ignore) {
                    Ok(true) => {}
                    _ => self.say("error ignore MAC-END".into()),
                }
            }
            (Some("offline"), Some(state), _) => self.runner.offline = state == "on",
            (Some("espnow"), Some(state), _) => {
                radio::ENABLED.store(state == "on", Ordering::Relaxed);
            }
            (Some("pay"), Some(address), Some(amount)) => self.pay(address, amount),
            (Some("pool"), Some("clear"), _) => self.runner.engine.clear_pool(),
            (Some("tx"), Some(id), _) => self.print_tx(id),
            (Some("trust"), Some(key), _) => match parse_key(key) {
                Some(key) => {
                    self.trust(key);
                    self.say("trusted".into());
                }
                None => self.say("error trust KEY".into()),
            },
            (Some("name"), Some(_), _) => {
                self.rename(line["name".len()..].trim());
                self.say("named".into());
            }
            (Some("clock"), Some(value), _) => match value.parse() {
                Ok(mhz) if clock::is_speed(mhz) => {
                    self.restart_with(|settings| settings.clock = format!("{mhz}"));
                }
                _ => self.say("error clock 80 160 240".into()),
            },
            (Some("full"), Some("on"), _) => self.restart_with(|settings| {
                settings.cap = "0".into();
                settings.clock = "240".into();
            }),
            (Some("full"), Some("off"), _) => self.restart_with(|settings| {
                settings.cap.clear();
                settings.clock.clear();
            }),
            (Some("network"), Some(state @ ("on" | "off")), _) => {
                self.restart_with(|settings| settings.own_network = state.into());
            }
            (Some("wifi"), Some("off"), _) => self.restart_with(|settings| {
                settings.ssid.clear();
                settings.password.clear();
            }),
            (Some("wifi"), Some(ssid), password) => self.restart_with(|settings| {
                settings.ssid = ssid.into();
                settings.password = password.unwrap_or("").into();
            }),
            (Some("dial"), Some("off"), _) => self.restart_with(|settings| settings.dial.clear()),
            (Some("dial"), Some(target), _) => {
                self.restart_with(|settings| settings.dial = target.into());
            }
            _ => println!(
                "commands: status, boots, screen, logo, view basic|detail|easy, lang en|ko, \
                 smooth FPS, bright 0-255, clock 80|160|240, full on|off, network on|off, power DBM, ignore MAC-END|none, \
                 pay ADDRESS AMOUNT, tx ID, pool clear, mine on|off, limit HASHES|default, \
                 offline on|off, espnow on|off, wifi SSID [PASSWORD], wifi off, dial IP:PORT, \
                 dial off, name NAME, trust KEY"
            ),
        }
    }

    fn pay(&mut self, address: &str, amount: &str) {
        let units = amount
            .parse::<f64>()
            .map(|value| (value * COIN as f64) as u64);
        let (Some(to), Ok(units)) = (parse_address(address), units) else {
            self.say("error pay ADDRESS AMOUNT".into());
            return;
        };
        let started = device_ms();
        match self.runner.engine.pay(to, units, 0) {
            Ok(id) => self.say(format!("paid {} in {}ms", hex(&id), device_ms() - started)),
            Err(error) => self.say(format!("error {error:?}")),
        }
    }

    /// Says where a transaction is: at a height on the best chain, waiting, or nowhere.
    /// Wi-Fi only takes new settings at start-up. The chain comes back from flash.
    fn restart_with(&mut self, change: impl FnOnce(&mut Settings)) {
        self.save(change);
        self.restart_ms = Some(device_ms() + RESTART_DELAY_MS);
    }

    /// Prints what was drawn and what the panel's own memory holds, row by row.
    fn print_screen(&mut self) {
        let mut same = self.screen.is_connected();
        println!(
            "screen connected={same} spilled={}",
            self.screen.take_spilled()
        );
        for page in 0..PAGES {
            let mut held = [0u8; WIDTH];
            let read = self.screen.read_row(page, &mut held).is_ok();
            let drawn = self.screen.row(page);
            same &= read && drawn == held;
            println!("row {page} {} {}", hex(drawn), hex(&held));
        }
        println!("screen matches={same}");
    }

    fn state(&self, now: u64) -> State {
        if let Some(percent) = self.control.update_percent(now) {
            State::Updating(percent)
        } else if let Some(height) = self.runner.engine.syncing_to() {
            State::Syncing(height)
        } else if self.memory_full {
            State::MemoryFull
        } else if self.runner.offline {
            State::Offline
        } else if self.mining {
            State::Mining
        } else {
            State::Idle
        }
    }

    fn progress(&self, state: State, height: u64, difficulty: u64) -> f32 {
        match state {
            State::Updating(percent) => f32::from(percent) / 100.0,
            State::Syncing(target) => height as f32 / target.max(1) as f32,
            _ => self.tip_attempts as f32 / difficulty.max(1) as f32,
        }
    }

    fn draw(&mut self, now: u64) {
        if now < self.splash_until_ms {
            self.screen_ms = now + TEXT_EVERY_MS;
            views::splash(&mut self.screen, &self.coins, now);
            return;
        }
        self.screen_ms = now + TEXT_EVERY_MS;
        let engine = &self.runner.engine;
        let status = engine.status();
        let state = self.state(now);
        let rules = engine.chain().rules();
        let coin = engine.chain().config();
        let interval = rules.retarget_interval.max(1);
        let genesis = engine.chain().genesis_id();
        let address = format_address(&engine.address());
        let info = Info {
            now_ms: now,
            state,
            name: &self.control.identity.name,
            coin: &coin.name,
            ticker: &coin.ticker,
            genesis: &genesis,
            address: &address,
            build: BUILD,
            height: status.height,
            balance: status.balance,
            hash_rate: u64::from(self.rate),
            limit: self.limit,
            temperature: self.temperature(),
            difficulty: status.difficulty,
            block_ms: rules.block_time_ms,
            tip: &status.tip,
            work: status.work,
            mined: self.mined,
            blocks_to_retarget: interval - (status.height + 1) % interval,
            block_age_ms: now.saturating_sub(self.tip_seen_ms),
            block_time_ms: self.block_time_ms,
            espnow: self.runner.peers_on(LINK_RADIO),
            wifi: self.runner.peers_on(LINK_WIFI),
            usb: self.runner.peers_on(LINK_USB),
            managed: self.control.is_managed(now),
            waiting: status.pool,
            free_memory: esp_alloc::HEAP.free(),
            log_left: self.store.log_left(),
            own_network: &self.network.own_name,
            router: self.network.router_address(),
            joining_router: self.network.joins_router,
            event: self.event,
            notes: &self.notes,
            progress: self.progress(state, status.height, status.difficulty),
        };
        views::draw(
            &mut self.screen,
            &self.coins,
            &info,
            self.view,
            self.language,
        );
    }
}
