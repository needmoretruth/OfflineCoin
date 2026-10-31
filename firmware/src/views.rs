//! What the OLED shows. Three views, each in English and Korean:
//! basic is the everyday view, detail is for checking the node, easy is for reading at a glance.
use crate::screen::{Font, Screen};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::f32::consts::TAU;
use core::net::Ipv4Addr;
use olc_core::coin::COIN;
use olc_node::Event;

const TICKER: &str = "OLC";
const LINE: i32 = 11;
const ROTATE_EVERY_MS: u64 = 3_000;
const EVENT_SHOWN_MS: u64 = 4_000;
const TURN_MS: u64 = 2_400;
const SPINNER: [&str; 4] = ["-", "\\", "|", "/"];
const SMALL_COIN_RADIUS: i32 = 19;
const SMALL_COIN_FRAMES: usize = 32;
const SOLID: f32 = 0.48;
const DIM: f32 = 0.45;

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

    /// The coin needs more frames than numbers do.
    pub fn redraw_ms(self) -> u64 {
        if self == View::Easy {
            160
        } else {
            300
        }
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

    fn pick<'a>(self, english: &'a str, korean: &'a str) -> &'a str {
        match self {
            Language::English => english,
            Language::Korean => korean,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum State {
    Mining,
    Idle,
    Offline,
    MemoryFull,
}

pub struct Info<'a> {
    pub now_ms: u64,
    pub state: State,
    pub height: u64,
    pub balance: u64,
    pub hash_rate: u64,
    pub difficulty: u64,
    pub tip: &'a [u8],
    pub work: u128,
    pub mined: u32,
    pub blocks_to_retarget: u64,
    pub espnow: usize,
    pub wifi: usize,
    pub usb: usize,
    pub waiting: usize,
    pub free_memory: usize,
    pub own_network: &'a str,
    pub router: Option<Ipv4Addr>,
    pub joining_router: bool,
    pub event: Option<(u64, Event)>,
    /// Hash attempts on this block, as a share of what a block takes on average.
    pub progress: f32,
}

impl Info<'_> {
    fn peers(&self) -> usize {
        self.espnow + self.wifi + self.usb
    }

    fn state_word(&self, language: Language) -> &'static str {
        match self.state {
            State::Mining => language.pick("MINING", "채굴 중"),
            State::Idle => language.pick("IDLE", "대기"),
            State::Offline => language.pick("OFFLINE", "연결 끊김"),
            State::MemoryFull => language.pick("FULL", "메모리 부족"),
        }
    }

    fn spinner(&self) -> &'static str {
        if self.state == State::Mining {
            SPINNER[(self.now_ms / 300) as usize % SPINNER.len()]
        } else {
            " "
        }
    }

    fn recent_event(&self) -> Option<Event> {
        let (at, event) = self.event?;
        (self.now_ms.saturating_sub(at) < EVENT_SHOWN_MS).then_some(event)
    }

    fn where_to_reach(&self, language: Language) -> String {
        match self.router {
            Some(address) => format!("IP {address}"),
            None if self.joining_router => language.pick("joining router", "공유기 접속 중").into(),
            None => format!("{} {}", language.pick("wifi", "와이파이"), self.own_network),
        }
    }

    fn tip_hex(&self, bytes: usize) -> String {
        self.tip[..bytes]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn uptime(&self, language: Language) -> String {
        let minutes = self.now_ms / 60_000;
        match language {
            Language::English => format!("{}h{:02}m", minutes / 60, minutes % 60),
            Language::Korean => format!("{}시간 {}분", minutes / 60, minutes % 60),
        }
    }
}

/// Three digits at most, with a decimal only when it says something.
fn short(value: u64) -> String {
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

pub fn coins(units: u64) -> String {
    format!("{}.{:02}", units / COIN, units % COIN / (COIN / 100))
}

fn event_text(event: Event, language: Language) -> String {
    match (language, event) {
        (Language::English, Event::Mined { height }) => format!("mined block {height}"),
        (Language::Korean, Event::Mined { height }) => format!("{height}번 블록 채굴"),
        (Language::English, Event::BlockAccepted { height }) => format!("got block {height}"),
        (Language::Korean, Event::BlockAccepted { height }) => format!("{height}번 블록 받음"),
        (Language::English, Event::Reorganized { height, .. }) => format!("new branch at {height}"),
        (Language::Korean, Event::Reorganized { height, .. }) => {
            format!("{height}번에서 체인 교체")
        }
        (Language::English, Event::TxAccepted) => "payment accepted".into(),
        (Language::Korean, Event::TxAccepted) => "송금 확인".into(),
        (Language::English, _) => "rejected".into(),
        (Language::Korean, _) => "거부함".into(),
    }
}

/// Whether a pixel of the coin is lit. Bright metal is solid and the rest is a thin dither, which
/// keeps the symbol readable on a panel with no greys.
fn coin_pixel(spin: olc_logo::Spin, x: i32, y: i32, radius: i32) -> bool {
    const BAYER: [[f32; 4]; 4] = [
        [0.03, 0.53, 0.16, 0.66],
        [0.78, 0.28, 0.91, 0.41],
        [0.22, 0.72, 0.09, 0.59],
        [0.97, 0.47, 0.84, 0.34],
    ];
    let (u, v) = (
        (x as f32 + 0.5) / radius as f32,
        -(y as f32 + 0.5) / radius as f32,
    );
    spin.shade(u, v).is_some_and(|light| {
        light > SOLID || light * DIM > BAYER[(y & 3) as usize][(x & 3) as usize]
    })
}

fn spin_at(now_ms: u64) -> f32 {
    (now_ms % TURN_MS) as f32 / TURN_MS as f32 * TAU
}

/// The coin of the easy view, drawn once at start-up, so spinning it costs the miner nothing.
pub struct SmallCoin(Vec<[u64; SMALL_COIN_RADIUS as usize * 2]>);

impl SmallCoin {
    pub fn new() -> Self {
        let frames = (0..SMALL_COIN_FRAMES).map(|frame| {
            let spin = olc_logo::Spin::new(frame as f32 / SMALL_COIN_FRAMES as f32 * TAU);
            let mut rows = [0u64; SMALL_COIN_RADIUS as usize * 2];
            for (row, bits) in rows.iter_mut().enumerate() {
                for column in 0..SMALL_COIN_RADIUS * 2 {
                    let (x, y) = (column - SMALL_COIN_RADIUS, row as i32 - SMALL_COIN_RADIUS);
                    if coin_pixel(spin, x, y, SMALL_COIN_RADIUS) {
                        *bits |= 1 << column;
                    }
                }
            }
            rows
        });
        Self(frames.collect())
    }

    fn draw(&self, screen: &mut Screen, now_ms: u64) {
        let frame = (now_ms % TURN_MS) as usize * SMALL_COIN_FRAMES / TURN_MS as usize;
        for (y, bits) in self.0[frame].iter().enumerate() {
            for x in 0..SMALL_COIN_RADIUS * 2 {
                if bits >> x & 1 == 1 {
                    screen.pixel(x, y as i32, true);
                }
            }
        }
    }
}

pub fn splash(screen: &mut Screen, now_ms: u64) {
    const RADIUS: i32 = 30;
    screen.clear();
    let spin = olc_logo::Spin::new(spin_at(now_ms));
    for y in -RADIUS..RADIUS {
        for x in -RADIUS..RADIUS {
            if coin_pixel(spin, x, y, RADIUS) {
                screen.pixel(RADIUS + x, 32 + y, true);
            }
        }
    }
    screen.text(68, 20, "Offline", Font::Regular);
    screen.text(68, 32, "Coin", Font::Regular);
    screen.flush();
}

fn basic(screen: &mut Screen, info: &Info, language: Language) {
    let number = match language {
        Language::English => format!("{TICKER} #{}", info.height),
        Language::Korean => format!("{TICKER} {}번", info.height),
    };
    screen.text(0, 0, &number, Font::Regular);
    let corner = format!("{} {}", info.state_word(language), info.spinner());
    screen.text_right(0, &corner, Font::Regular);
    screen.text(
        0,
        LINE,
        &format!("{} {TICKER}", coins(info.balance)),
        Font::Regular,
    );
    let (difficulty, espnow, wifi, usb) = match language {
        Language::English => ("d", "espnow", "wifi", "usb"),
        Language::Korean => ("난이도", "무선", "와이파이", "유선"),
    };
    let rate = format!(
        "{} H/s {difficulty} {}",
        short(info.hash_rate),
        short(info.difficulty)
    );
    screen.text(0, LINE * 2, &rate, Font::Regular);
    let peers = format!(
        "{espnow} {} {wifi} {} {usb} {}",
        info.espnow, info.wifi, info.usb
    );
    screen.text(0, LINE * 3, &peers, Font::Regular);
    let last = match info.recent_event() {
        Some(event) => event_text(event, language),
        None if (info.now_ms / ROTATE_EVERY_MS).is_multiple_of(2) => {
            format!("{} {}", language.pick("tip", "최신"), info.tip_hex(7))
        }
        None => info.where_to_reach(language),
    };
    screen.text(0, LINE * 4, &last, Font::Regular);
    screen.bar(58, 6, info.progress);
}

fn detail_english(screen: &mut Screen, info: &Info) {
    let reach = info
        .router
        .map_or(String::from(info.own_network), |address| {
            format!("{address}")
        });
    let lines = [
        format!(
            "#{} {} {} H/s",
            info.height,
            info.state_word(Language::English),
            short(info.hash_rate)
        ),
        format!("tip {}", info.tip_hex(10)),
        format!("diff {}  next {}", info.difficulty, info.blocks_to_retarget),
        format!("work {}  mined {}", short(info.work as u64), info.mined),
        format!("bal {} {TICKER}", coins(info.balance)),
        format!("espnow {} wifi {} usb {}", info.espnow, info.wifi, info.usb),
        format!(
            "wait {}  free {}",
            info.waiting,
            short(info.free_memory as u64)
        ),
        format!("up {}  {reach}", info.uptime(Language::English)),
    ];
    for (index, line) in lines.iter().enumerate() {
        screen.text(0, index as i32 * 8, line, Font::Small);
    }
}

fn detail_korean(screen: &mut Screen, info: &Info) {
    let reach = info
        .router
        .map_or(String::from(info.own_network), |address| {
            format!("{address}")
        });
    let lines = [
        format!(
            "{}번 {} {}",
            info.height,
            info.state_word(Language::Korean),
            short(info.hash_rate)
        ),
        format!("최신 {}", info.tip_hex(8)),
        format!(
            "난이도 {} 조정 {}",
            info.difficulty, info.blocks_to_retarget
        ),
        format!("누적 {} 채굴 {}", short(info.work as u64), info.mined),
        format!(
            "대기 {} 여유 {}",
            info.waiting,
            short(info.free_memory as u64)
        ),
        format!("{} {reach}", info.uptime(Language::Korean)),
    ];
    for (index, line) in lines.iter().enumerate() {
        screen.text(0, index as i32 * 10 + 2, line, Font::Regular);
    }
}

fn easy(screen: &mut Screen, small_coin: &SmallCoin, info: &Info, language: Language) {
    small_coin.draw(screen, info.now_ms);
    let whole = info.balance / COIN;
    let amount = if whole < 10_000 {
        coins(info.balance)
    } else {
        short(whole)
    };
    screen.text(44, 0, &amount, Font::Big);
    screen.text(44, 21, TICKER, Font::Regular);
    let next = info.height + 1;
    let doing = match (info.state, language) {
        (State::Mining, Language::English) => format!("Mining block {next}"),
        (State::Mining, Language::Korean) => format!("{next}번 블록 채굴 중"),
        (State::Idle, _) => language.pick("Mining is off", "채굴 꺼짐").into(),
        (State::Offline, _) => language.pick("Offline", "연결 끊김").into(),
        (State::MemoryFull, _) => language.pick("Memory is full", "메모리 부족").into(),
    };
    screen.text(0, 39, &doing, Font::Reading);
    let around = match (info.recent_event(), info.peers(), language) {
        (Some(event), _, _) => event_text(event, language),
        (None, 0, _) => language.pick("No nodes nearby", "연결된 노드 없음").into(),
        (None, 1, Language::English) => "1 node connected".into(),
        (None, peers, Language::English) => format!("{peers} nodes connected"),
        (None, peers, Language::Korean) => format!("노드 {peers}대 연결됨"),
    };
    screen.text(0, 51, &around, Font::Reading);
}

pub fn draw(
    screen: &mut Screen,
    small_coin: &SmallCoin,
    info: &Info,
    view: View,
    language: Language,
) {
    screen.clear();
    match (view, language) {
        (View::Basic, _) => basic(screen, info, language),
        (View::Detail, Language::English) => detail_english(screen, info),
        (View::Detail, Language::Korean) => detail_korean(screen, info),
        (View::Easy, _) => easy(screen, small_coin, info, language),
    }
    screen.flush();
}
