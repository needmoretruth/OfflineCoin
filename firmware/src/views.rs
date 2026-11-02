//! What the OLED shows. Three views, each in English and Korean:
//! basic is the everyday view, detail is for checking the node, easy is for reading at a glance.
use crate::pages;
use crate::screen::{self, Font, Screen};
use crate::words::{note_text, state_sentence, state_word};
use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::String;
use core::net::Ipv4Addr;
use olc_core::amount::format_coins;
use olc_core::coin::COIN;
use olc_logo::Reel;
use olc_node::Event;

const LINE: i32 = 11;
const LAST_ROW: i32 = 39 + LINE;
const ROTATE_EVERY_MS: u64 = 3_000;
const EVENT_SHOWN_MS: u64 = 4_000;
const SPINNER: [&str; 4] = ["-", "\\", "|", "/"];
const SMALL_COIN_RADIUS: i32 = 19;
const LARGE_COIN_RADIUS: i32 = 30;
const LARGE_COIN_TOP: i32 = 2;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum View {
    Basic,
    Detail,
    Easy,
}

impl View {
    pub fn next(self) -> Self {
        match self {
            View::Basic => View::Detail,
            View::Detail => View::Easy,
            View::Easy => View::Basic,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            View::Basic => "basic",
            View::Detail => "detail",
            View::Easy => "easy",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        [View::Basic, View::Detail, View::Easy]
            .into_iter()
            .find(|view| view.name() == name)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Language {
    English,
    Korean,
}

impl Language {
    pub fn name(self) -> &'static str {
        match self {
            Language::English => "en",
            Language::Korean => "ko",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        [Language::English, Language::Korean]
            .into_iter()
            .find(|language| language.name() == name)
    }
}

/// What the board is doing, most urgent first.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum State {
    Updating(u8),
    Cooling,
    Syncing(u64),
    MemoryFull,
    Offline,
    Mining,
    Idle,
}

/// Something that happened, to show for a moment and keep in the detail view.
#[derive(Clone, Copy)]
pub enum Note {
    Chain(Event),
    Order(&'static str),
    Updated(&'static str),
}

pub struct Info<'a> {
    pub now_ms: u64,
    pub state: State,
    pub name: &'a str,
    pub coin: &'a str,
    pub ticker: &'a str,
    pub genesis: &'a [u8],
    pub address: &'a str,
    pub build: &'a str,
    pub height: u64,
    pub balance: u64,
    pub hash_rate: u64,
    pub limit: u32,
    pub temperature: f32,
    pub difficulty: u64,
    pub block_ms: u64,
    pub tip: &'a [u8],
    pub work: u128,
    pub mined: u32,
    pub blocks_to_retarget: u64,
    /// Since this board saw the tip change.
    pub block_age_ms: u64,
    /// The mean time between the last few blocks.
    pub block_time_ms: Option<u64>,
    pub espnow: usize,
    pub wifi: usize,
    pub usb: usize,
    pub managed: bool,
    pub waiting: usize,
    pub free_memory: usize,
    pub log_left: u32,
    pub own_network: &'a str,
    pub router: Option<Ipv4Addr>,
    pub joining_router: bool,
    pub event: Option<(u64, Note)>,
    pub notes: &'a VecDeque<Note>,
    /// How far the bar under the basic view goes.
    pub progress: f32,
}

impl Info<'_> {
    fn peers(&self) -> usize {
        self.espnow + self.wifi + self.usb
    }

    fn spinner(&self) -> &'static str {
        if self.state == State::Mining {
            SPINNER[(self.now_ms / 300) as usize % SPINNER.len()]
        } else {
            " "
        }
    }

    fn recent_event(&self) -> Option<Note> {
        let (at, note) = self.event?;
        (self.now_ms.saturating_sub(at) < EVENT_SHOWN_MS).then_some(note)
    }

    pub fn where_to_reach(&self, language: Language) -> String {
        match self.router {
            Some(address) => format!("IP {address}"),
            None if self.joining_router => language.pick("joining router", "공유기 접속 중").into(),
            None if self.own_network.is_empty() => {
                language.pick("wifi off", "와이파이 꺼짐").into()
            }
            None => format!("{} {}", language.pick("wifi", "와이파이"), self.own_network),
        }
    }

    pub fn hex(&self, bytes: &[u8], count: usize) -> String {
        bytes[..count]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    pub fn tip_hex(&self, bytes: usize) -> String {
        self.hex(self.tip, bytes)
    }

    /// The hash rate, with the cap beside it when there is one.
    pub fn rate_text(&self) -> String {
        match self.limit {
            0 => format!("{} H/s", short(self.hash_rate)),
            limit => format!("{}/{}", short(self.hash_rate), short(u64::from(limit))),
        }
    }

    /// This board's rate against what the difficulty says the whole network does.
    pub fn share_percent(&self) -> u64 {
        let network = self.difficulty.saturating_mul(1_000) / self.block_ms.max(1);
        (self.hash_rate * 100 / network.max(1)).min(100)
    }
}

/// Three digits at most, with a decimal only when it says something.
pub fn short(value: u64) -> String {
    let (unit, letter) = match value {
        0..1_000 => return format!("{value}"),
        1_000..1_000_000 => (1_000, "k"),
        1_000_000..1_000_000_000 => (1_000_000, "M"),
        _ => (1_000_000_000, "G"),
    };
    let (whole, tenth) = (value / unit, value / (unit / 10) % 10);
    if whole >= 100 || tenth == 0 {
        format!("{whole}{letter}")
    } else {
        format!("{whole}.{tenth}{letter}")
    }
}

/// An amount as the app writes it, with as many decimals as `room` pixels hold. 128 pixels take
/// 21 letters of the regular font, so a line with a height beside it often loses a few. It cuts
/// and never rounds up, and only an amount too long for its whole part shrinks to `1.2M`.
pub fn coins_within(units: u64, font: Font, room: i32) -> String {
    for unit in (0..=8).map(|power| 10u64.pow(power)) {
        let text = format_coins(units - units % unit);
        if screen::width(&text, font) <= room {
            return text;
        }
    }
    short(units / COIN)
}

/// Whether a pixel of the coin is lit. Bright metal is solid and the rest is a thin dither, which
/// keeps the symbol readable on a panel with no greys.
/// The coin in both sizes. Drawing them is spare-time work, so the start does not wait for it.
pub struct Coins {
    small: Reel,
    large: Reel,
}

impl Coins {
    pub fn new() -> Self {
        Self {
            small: Reel::new(SMALL_COIN_RADIUS),
            large: Reel::new(LARGE_COIN_RADIUS),
        }
    }

    /// Enough of the logo to show the first screen.
    pub fn first_frame(&mut self) {
        self.large.work(LARGE_COIN_RADIUS as usize * 2);
    }

    pub fn work_on_logo(&mut self, rows: usize) {
        self.large.work(rows);
    }

    pub fn work(&mut self, rows: usize) {
        if self.small.is_full() {
            self.large.work(rows);
        } else {
            self.small.work(rows);
        }
    }

    fn draw_small(&self, screen: &mut Screen, now_ms: u64) {
        let width = self.small.width();
        if let Some(rows) = self.small.frame(olc_logo::frame_at(now_ms)) {
            screen.stamp(0, 0, width, rows);
        }
    }

    fn draw_large(&self, screen: &mut Screen, now_ms: u64) {
        let width = self.large.width();
        if let Some(rows) = self.large.frame(olc_logo::frame_at(now_ms)) {
            screen.stamp(0, LARGE_COIN_TOP, width, rows);
        }
    }

    /// Only the coin changes, so a frame costs a few rows on the wire.
    pub fn next_frame(&self, screen: &mut Screen, now_ms: u64, splash: bool) {
        if splash {
            self.draw_large(screen, now_ms);
        } else {
            self.draw_small(screen, now_ms);
        }
        screen.flush();
    }
}

pub fn splash(screen: &mut Screen, coins: &Coins, now_ms: u64) {
    draw_splash(screen, coins, now_ms);
    screen.flush();
}

/// The start screen with a line that fills while the chain loads.
pub fn loading(screen: &mut Screen, coins: &Coins, now_ms: u64, done: usize, total: usize) {
    const LEFT: i32 = 68;
    const LENGTH: i32 = 54;
    const TOP: i32 = 48;
    draw_splash(screen, coins, now_ms);
    let filled = (done * LENGTH as usize / total.max(1)) as i32;
    for x in LEFT..LEFT + LENGTH {
        screen.pixel(x, TOP, true);
        screen.pixel(x, TOP + 1, x < LEFT + filled);
        screen.pixel(x, TOP + 2, x < LEFT + filled);
        screen.pixel(x, TOP + 3, true);
    }
    screen.text(LEFT, TOP + 5, &format!("{done}/{total}"), Font::Regular);
    screen.flush();
}

fn draw_splash(screen: &mut Screen, coins: &Coins, now_ms: u64) {
    screen.clear();
    screen.shift_for(now_ms);
    coins.draw_large(screen, now_ms);
    screen.text(68, 20, "Offline", Font::Regular);
    screen.text(68, 32, "Coin", Font::Regular);
}

fn basic(screen: &mut Screen, info: &Info, language: Language) {
    let corner = format!("{} {}", state_word(info.state, language), info.spinner());
    let room = screen.text_right(0, &corner, Font::Regular);
    let name = if info.name.is_empty() {
        info.ticker
    } else {
        info.name
    };
    screen.text_before(0, 0, name, Font::Regular, room - 6);
    let number = match language {
        Language::English => format!("#{}", info.height),
        Language::Korean => format!("{}번", info.height),
    };
    let around = format!("{number}  {}", info.ticker);
    let room = screen::ROOM - screen::width(&around, Font::Regular);
    let amount = coins_within(info.balance, Font::Regular, room);
    let money = format!("{number} {amount} {}", info.ticker);
    screen.text(0, LINE, &money, Font::Regular);
    let rate = match (language, info.limit) {
        (Language::English, _) => format!("{} d {}", info.rate_text(), short(info.difficulty)),
        (Language::Korean, 0) => format!("{} 난이도 {}", info.rate_text(), short(info.difficulty)),
        (Language::Korean, _) => format!("{} 난 {}", info.rate_text(), short(info.difficulty)),
    };
    screen.text(0, LINE * 2, &rate, Font::Regular);
    let (espnow, wifi, usb) = match language {
        Language::English => ("espnow", "wifi", "usb"),
        Language::Korean => ("무선", "와이파이", "유선"),
    };
    let peers = format!(
        "{espnow} {} {wifi} {} {usb} {}",
        info.espnow, info.wifi, info.usb
    );
    screen.text(0, LINE * 3, &peers, Font::Regular);
    let last = match info.recent_event() {
        Some(note) => note_text(note, language),
        None if (info.now_ms / ROTATE_EVERY_MS).is_multiple_of(2) => {
            format!("{} {}", language.pick("tip", "최신"), info.tip_hex(7))
        }
        None => info.where_to_reach(language),
    };
    screen.text(0, LINE * 4, &last, Font::Regular);
    screen.bar(58, 5, info.progress);
}

fn easy(screen: &mut Screen, reels: &Coins, info: &Info, language: Language) {
    reels.draw_small(screen, info.now_ms);
    let amount = coins_within(info.balance, Font::Big, screen::ROOM - 44);
    screen.text(44, 0, &amount, Font::Big);
    let end = screen.text(44, 21, info.ticker, Font::Regular);
    screen.text_before(end + 6, 21, info.name, Font::Regular, screen::ROOM);
    let doing = state_sentence(info.state, info.height, language);
    screen.text(0, 38, &doing, Font::Reading);
    let around = match (info.recent_event(), info.peers(), language) {
        (Some(note), _, _) => note_text(note, language),
        (None, 0, _) => language.pick("No nodes nearby", "연결된 노드 없음").into(),
        (None, 1, Language::English) => "1 node connected".into(),
        (None, peers, Language::English) => format!("{peers} nodes connected"),
        (None, peers, Language::Korean) => format!("노드 {peers}대 연결됨"),
    };
    screen.text_fitting(
        LAST_ROW,
        &around,
        &[Font::Reading, Font::Regular, Font::Small],
    );
}

pub fn draw(screen: &mut Screen, coins: &Coins, info: &Info, view: View, language: Language) {
    screen.clear();
    screen.shift_for(info.now_ms);
    match view {
        View::Basic => basic(screen, info, language),
        View::Detail => pages::draw(screen, info, language),
        View::Easy => easy(screen, coins, info, language),
    }
    screen.flush();
}
