//! The detail view: pages that turn by themselves. English fits seven lines in the small font,
//! Korean six in the regular one, because its letters are taller.
use crate::screen::{width, Font, Screen, ROOM};
use crate::views::{coins, short, Info, Language, State};
use crate::words::{note_text, seconds, state_word, uptime};
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

const PAGE_EVERY_MS: u64 = 5_000;
const PAGES: u64 = 4;

fn head(info: &Info, language: Language, font: Font) -> String {
    let word = state_word(info.state, language);
    let height = match language {
        Language::English => format!("#{}", info.height),
        Language::Korean => format!("{}번", info.height),
    };
    if matches!(info.state, State::Updating(_) | State::Syncing(_)) {
        return format!("{height} {word}");
    }
    let rate = info.rate_text();
    [rate.as_str(), rate.trim_end_matches(" H/s")]
        .iter()
        .map(|rate| format!("{height} {word} {rate}"))
        .find(|line| width(line, font) <= ROOM)
        .unwrap_or_else(|| format!("{height} {word}"))
}

fn chain_page(info: &Info, language: Language) -> Vec<String> {
    let age = seconds(info.block_age_ms, language);
    let average = info
        .block_time_ms
        .map_or(String::from("-"), |time| seconds(time, language));
    let (retarget, work) = (info.blocks_to_retarget, short(info.work as u64));
    match language {
        Language::English => vec![
            format!("tip {}", info.tip_hex(10)),
            format!("diff {}  next {retarget}", info.difficulty),
            format!("last {age}  avg {average}"),
            format!("work {work}  mined {}", info.mined),
        ],
        Language::Korean => vec![
            format!("최신 {}", info.tip_hex(8)),
            format!("난이도 {} 조정 {retarget}", info.difficulty),
            format!("마지막 {age} 평균 {average}"),
            format!("누적 {work} 채굴 {}", info.mined),
        ],
    }
}

fn network_page(info: &Info, language: Language) -> Vec<String> {
    let reach = info.where_to_reach(language);
    let (espnow, wifi, usb) = (info.espnow, info.wifi, info.usb);
    let share = info.share_percent();
    let genesis = |bytes| info.hex(info.genesis, bytes);
    match language {
        Language::English => vec![
            format!("coin {} {}", info.coin, genesis(4)),
            format!("name {}", info.name),
            format!("espnow {espnow} wifi {wifi} usb {usb}"),
            reach,
            String::from(if info.managed {
                "managed by radio"
            } else {
                "not managed by radio"
            }),
            format!("share {share}%"),
        ],
        Language::Korean => vec![
            format!("코인 {} {}", info.coin, genesis(2)),
            format!("이름 {}", info.name),
            format!("무선 {espnow} 와이파이 {wifi} 유선 {usb}"),
            reach,
            format!(
                "{} 점유 {share}%",
                if info.managed {
                    "원격 관리"
                } else {
                    "관리 없음"
                }
            ),
        ],
    }
}

fn node_page(info: &Info, language: Language) -> Vec<String> {
    let (mined, waiting) = (info.mined, info.waiting);
    let balance = format!("{} {}", coins(info.balance), info.ticker);
    let free = short(info.free_memory as u64);
    let log = short(u64::from(info.log_left));
    let time = uptime(info.now_ms, language);
    let cap = |none| match info.limit {
        0 => String::from(none),
        limit => short(u64::from(limit)),
    };
    match language {
        Language::English => vec![
            format!("addr {}", info.address.get(..20).unwrap_or(info.address)),
            format!("bal {balance}"),
            format!("mined {mined}  wait {waiting}  cap {}", cap("none")),
            format!("free {free}  log {log}"),
            format!("up {time}  build {}", info.build),
        ],
        Language::Korean => vec![
            format!("주소 {}", info.address.get(..15).unwrap_or(info.address)),
            format!("잔액 {balance}"),
            format!("채굴 {mined} 대기 {waiting} 상한 {}", cap("없음")),
            format!("여유 {free} 기록 {log}"),
            format!("{time} 빌드 {}", info.build),
        ],
    }
}

fn notes_page(info: &Info, language: Language) -> Vec<String> {
    if info.notes.is_empty() {
        return vec![language.pick("nothing yet", "기록 없음").into()];
    }
    let newest_first = info.notes.iter().rev();
    newest_first
        .map(|note| note_text(*note, language))
        .collect()
}

pub fn draw(screen: &mut Screen, info: &Info, language: Language) {
    let body = match info.now_ms / PAGE_EVERY_MS % PAGES {
        0 => chain_page(info, language),
        1 => network_page(info, language),
        2 => node_page(info, language),
        _ => notes_page(info, language),
    };
    let (font, step, top) = match language {
        Language::English => (Font::Small, 9, 0),
        Language::Korean => (Font::Regular, 10, 2),
    };
    screen.text(0, top, &head(info, language, font), font);
    for (index, line) in body.iter().enumerate() {
        screen.text(0, top + (index as i32 + 1) * step, line, font);
    }
}
