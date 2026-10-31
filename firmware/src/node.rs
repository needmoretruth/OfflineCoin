//! One pass of the board's work: move messages, mine a little, save, redraw.
use crate::radio;
use crate::screen::{Screen, PAGES, WIDTH};
use crate::store::{Settings, Store};
use crate::usb::Commands;
use crate::views::{self, coins, Info, Language, SmallCoin, State, View};
use crate::wifi::Network;
use alloc::format;
use alloc::string::String;
use core::sync::atomic::Ordering;
use esp_hal::gpio::Input;
use esp_hal::time::Instant;
use esp_println::println;
use olc_core::coin::COIN;
use olc_core::Hash;
use olc_node::{Event, Runner};
use olc_wallet::{format_address, parse_address};

/// The order links are added in, which is how peers are counted per link.
pub const LINK_RADIO: usize = 0;
pub const LINK_USB: usize = 1;
pub const LINK_WIFI: usize = 2;

/// About 13 ms of work, short enough to answer the radio between batches.
const MINING_BATCH: u32 = 200;
const STATUS_EVERY_MS: u64 = 10_000;
pub const SPLASH_LENGTH_MS: u64 = 2_500;
const SPLASH_REDRAW_MS: u64 = 80;
const BUTTON_DEBOUNCE_MS: u64 = 30;
const BUTTON_HOLD_MS: u64 = 800;
/// Mining stops here, so a full memory ends in a message and not a crash.
const MIN_FREE_HEAP: usize = 256 * 1024;

pub fn device_ms() -> u64 {
    Instant::now().duration_since_epoch().as_millis()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub struct Node {
    pub runner: Runner,
    pub store: Store,
    pub screen: Screen,
    pub commands: Commands,
    pub network: Network,
    pub small_coin: SmallCoin,
    /// The BOOT button: a press changes the view, a long press changes the language.
    pub button: Input<'static>,
    pub view: View,
    pub language: Language,
    pub mining: bool,
    /// Hashes per second, 0 for no limit.
    pub limit: u32,
    pub memory_full: bool,
    pub attempts: u32,
    pub rate: u32,
    pub window_ms: u64,
    /// Attempts since the tip last changed, which is what the bar on the screen shows.
    pub tip_attempts: u64,
    pub tip_seen: Hash,
    pub mined: u32,
    pub event: Option<(u64, Event)>,
    pub screen_ms: u64,
    pub status_ms: u64,
    pub pressed_ms: Option<u64>,
    /// The spinning logo shows until this time, which is on every start.
    pub splash_until_ms: u64,
}

impl Node {
    /// Returns true when it did mining work, false when the caller may sleep.
    pub fn turn(&mut self) -> bool {
        let now = device_ms();
        self.runner.step(now);
        while let Some(line) = self.next_command() {
            self.run_command(&line);
        }
        self.read_button(now);
        if now.saturating_sub(self.window_ms) >= 1_000 {
            self.rate = self.attempts;
            self.attempts = 0;
            self.window_ms = now;
        }
        if esp_alloc::HEAP.free() < MIN_FREE_HEAP {
            self.mining = false;
            self.memory_full = true;
        }
        let worked = self.mining && (self.limit == 0 || self.attempts < self.limit);
        if worked {
            self.runner.engine.mine(MINING_BATCH, now);
            self.attempts += MINING_BATCH;
            self.tip_attempts += u64::from(MINING_BATCH);
        }
        self.handle_events(now);
        while let Some(block) = self.runner.engine.take_fresh_block() {
            self.store.append(&block);
        }
        let tip = self.runner.engine.chain().tip();
        if tip != self.tip_seen {
            self.tip_seen = tip;
            self.tip_attempts = 0;
        }
        if now >= self.screen_ms {
            self.draw(now);
        }
        if now >= self.status_ms {
            self.status_ms = now + STATUS_EVERY_MS;
            self.print_status();
        }
        worked
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
                    println!("mined {height}");
                }
                Event::BlockAccepted { height } => println!("block {height}"),
                Event::Reorganized { replaced, height } => {
                    println!("reorg {height} replaced={replaced}");
                }
                Event::BlockRejected { reason } => println!("rejected block {reason:?}"),
                Event::TxAccepted => println!("payment accepted"),
                Event::TxRejected { reason } => println!("rejected payment {reason:?}"),
            }
            self.event = Some((now, event));
        }
    }

    fn run_command(&mut self, line: &str) {
        let mut words = line.split_whitespace();
        match (words.next(), words.next(), words.next()) {
            (Some("status"), _, _) => self.print_status(),
            (Some("screen"), _, _) => self.print_screen(),
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
            (Some("limit"), Some(value), _) => self.limit = value.parse().unwrap_or(self.limit),
            (Some("offline"), Some(state), _) => self.runner.offline = state == "on",
            (Some("espnow"), Some(state), _) => {
                radio::ENABLED.store(state == "on", Ordering::Relaxed);
            }
            (Some("pay"), Some(address), Some(amount)) => self.pay(address, amount),
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
                "commands: status, screen, logo, view basic|detail|easy, lang en|ko, \
                 pay ADDRESS AMOUNT, mine on|off, limit HASHES, offline on|off, espnow on|off, \
                 wifi SSID [PASSWORD], wifi off, dial IP:PORT, dial off"
            ),
        }
    }

    fn pay(&mut self, address: &str, amount: &str) {
        let units = amount
            .parse::<f64>()
            .map(|value| (value * COIN as f64) as u64);
        let (Some(to), Ok(units)) = (parse_address(address), units) else {
            println!("error pay ADDRESS AMOUNT");
            return;
        };
        let started = device_ms();
        match self.runner.engine.pay(to, units, 0) {
            Ok(id) => println!("paid {} in {}ms", hex(&id), device_ms() - started),
            Err(error) => println!("error {error:?}"),
        }
    }

    /// Wi-Fi only takes new settings at start-up. The chain comes back from flash.
    fn restart_with(&mut self, change: impl FnOnce(&mut Settings)) {
        self.save(change);
        esp_hal::system::software_reset();
    }

    fn print_status(&self) {
        let status = self.runner.engine.status();
        println!(
            "status height={} tip={} difficulty={} balance={} peers={} espnow={} usb={} wifi={} \
             pool={} hashrate={} mining={} view={} lang={} screen={} free={} router={:?} address={}",
            status.height,
            hex(&status.tip),
            status.difficulty,
            coins(status.balance),
            status.peers,
            self.runner.peers_on(LINK_RADIO),
            self.runner.peers_on(LINK_USB),
            self.runner.peers_on(LINK_WIFI),
            status.pool,
            self.rate,
            self.mining,
            self.view.name(),
            self.language.name(),
            self.screen.is_connected(),
            esp_alloc::HEAP.free(),
            self.network.router_address(),
            format_address(&self.runner.engine.address()),
        );
    }

    /// Prints what was drawn and what the panel's own memory holds, row by row.
    fn print_screen(&mut self) {
        let mut same = self.screen.is_connected();
        println!("screen connected={same}");
        for page in 0..PAGES {
            let mut held = [0u8; WIDTH];
            let read = self.screen.read_row(page, &mut held).is_ok();
            let drawn = self.screen.row(page);
            same &= read && drawn == held;
            println!("row {page} {} {}", hex(drawn), hex(&held));
        }
        println!("screen matches={same}");
    }

    fn state(&self) -> State {
        if self.memory_full {
            State::MemoryFull
        } else if self.runner.offline {
            State::Offline
        } else if self.mining {
            State::Mining
        } else {
            State::Idle
        }
    }

    fn draw(&mut self, now: u64) {
        if now < self.splash_until_ms {
            self.screen_ms = now + SPLASH_REDRAW_MS;
            views::splash(&mut self.screen, now);
            return;
        }
        self.screen_ms = now + self.view.redraw_ms();
        let engine = &self.runner.engine;
        let status = engine.status();
        let interval = engine.chain().rules().retarget_interval.max(1);
        let info = Info {
            now_ms: now,
            state: self.state(),
            height: status.height,
            balance: status.balance,
            hash_rate: u64::from(self.rate),
            difficulty: status.difficulty,
            tip: &status.tip,
            work: status.work,
            mined: self.mined,
            blocks_to_retarget: interval - (status.height + 1) % interval,
            espnow: self.runner.peers_on(LINK_RADIO),
            wifi: self.runner.peers_on(LINK_WIFI),
            usb: self.runner.peers_on(LINK_USB),
            waiting: status.pool,
            free_memory: esp_alloc::HEAP.free(),
            own_network: &self.network.own_name,
            router: self.network.router_address(),
            joining_router: self.network.joins_router,
            event: self.event,
            progress: self.tip_attempts as f32 / status.difficulty.max(1) as f32,
        };
        views::draw(
            &mut self.screen,
            &self.small_coin,
            &info,
            self.view,
            self.language,
        );
    }
}
