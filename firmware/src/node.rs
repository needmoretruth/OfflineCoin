//! One pass of the board's work: move messages, mine a little, save, redraw.
use crate::control::{cable_is_busy, parse_key, Control, BUILD};
use crate::radio;
use crate::screen::{Screen, PAGES, WIDTH};
use crate::store::{Settings, Store};
use crate::usb::Commands;
use crate::views::{self, coins, Info, Language, SmallCoin, State, View};
use crate::wifi::Network;
use alloc::collections::VecDeque;
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
const MAX_DROPPED: usize = 8;
pub const SPLASH_LENGTH_MS: u64 = 2_500;
const SPLASH_REDRAW_MS: u64 = 80;
const BUTTON_DEBOUNCE_MS: u64 = 30;
const BUTTON_HOLD_MS: u64 = 800;
/// Mining stops here, so a full memory ends in a message and not a crash.
const MIN_FREE_HEAP: usize = 256 * 1024;
/// A line on the cable counts as finished this long after its last byte.
pub const CABLE_QUIET_MS: u32 = 20;

pub fn device_ms() -> u64 {
    Instant::now().duration_since_epoch().as_millis()
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn decode_hash(text: &str, out: &mut Hash) -> bool {
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

pub struct Node {
    pub runner: Runner,
    pub control: Control,
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
    /// Hashes per second, 0 for no limit. Saved, so a cap outlives a restart.
    pub limit: u32,
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
        self.serve_control(now);
        while let Some(line) = self.next_command() {
            self.run_command(&line);
        }
        self.read_button(now);
        let window = now.saturating_sub(self.window_ms);
        if window >= 1_000 {
            self.rate = (u64::from(self.attempts) * 1_000 / window) as u32;
            self.attempts = 0;
            self.window_ms = now;
        }
        if esp_alloc::HEAP.free() < MIN_FREE_HEAP {
            self.mining = false;
            self.memory_full = true;
        }
        let batch = match self.limit {
            0 => MINING_BATCH,
            limit => MINING_BATCH.min(limit.saturating_sub(self.attempts)),
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

    fn set_limit(&mut self, limit: u32) {
        if limit != self.limit {
            self.limit = limit;
            self.save(|settings| settings.limit = format!("{limit}"));
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
            self.event = Some((now, event));
        }
    }

    pub fn run_command(&mut self, line: &str) {
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
            (Some("limit"), Some(value), _) => {
                if let Ok(limit) = value.parse() {
                    self.set_limit(limit);
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
                 pay ADDRESS AMOUNT, tx ID, pool clear, mine on|off, limit HASHES, \
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
    pub fn print_tx(&mut self, text: &str) {
        let mut id: Hash = [0; 32];
        let engine = &self.runner.engine;
        let line = match decode_hash(text, &mut id) {
            false => "error tx ID".into(),
            true => match engine.chain().find_tx(&id) {
                Some(height) => format!("tx {text} height={height}"),
                None if engine.in_pool(&id) => format!("tx {text} pool"),
                None => match self.dropped.iter().find(|(known, _)| *known == id) {
                    Some((_, reason)) => format!("tx {text} dropped={reason}"),
                    None => format!("tx {text} none"),
                },
            },
        };
        self.say(line);
    }

    /// Wi-Fi only takes new settings at start-up. The chain comes back from flash.
    fn restart_with(&mut self, change: impl FnOnce(&mut Settings)) {
        self.save(change);
        esp_hal::system::software_reset();
    }

    pub fn print_status(&mut self) {
        let status = self.runner.engine.status();
        let line = format!(
            "status height={} tip={} work={} difficulty={} balance={} peers={} espnow={} usb={} \
             wifi={} pool={} hashrate={} limit={} mining={} offline={} view={} lang={} screen={} \
             free={} router={:?} address={} reorg={}/{} build={BUILD} slot={} key={} name={}",
            status.height,
            hex(&status.tip),
            status.work,
            status.difficulty,
            coins(status.balance),
            status.peers,
            self.runner.peers_on(LINK_RADIO),
            self.runner.peers_on(LINK_USB),
            self.runner.peers_on(LINK_WIFI),
            status.pool,
            self.rate,
            self.limit,
            self.mining,
            self.runner.offline,
            self.view.name(),
            self.language.name(),
            self.screen.is_connected(),
            esp_alloc::HEAP.free(),
            self.network.router_address(),
            format_address(&self.runner.engine.address()),
            self.last_reorg.0,
            self.last_reorg.1,
            self.control.slot,
            self.control.gate.has_key(),
            self.control.identity.name,
        );
        self.say(line);
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
