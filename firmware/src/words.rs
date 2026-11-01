//! What the panel says, in English and Korean.
use crate::views::{coins, Language, Note, State};
use alloc::format;
use alloc::string::String;
use olc_core::ChainError;
use olc_node::Event;

impl Language {
    pub fn pick<'a>(self, english: &'a str, korean: &'a str) -> &'a str {
        match self {
            Language::English => english,
            Language::Korean => korean,
        }
    }
}

/// The word in a corner of the basic view and the top of every detail page.
pub fn state_word(state: State, language: Language) -> String {
    match state {
        State::Updating(percent) => format!("{} {percent}%", language.pick("UPDATE", "업데이트")),
        State::Syncing(height) => format!("{} {height}", language.pick("SYNC", "동기화")),
        State::MemoryFull => language.pick("FULL", "메모리 부족").into(),
        State::Offline => language.pick("OFFLINE", "연결 끊김").into(),
        State::Mining => language.pick("MINING", "채굴 중").into(),
        State::Idle => language.pick("IDLE", "대기").into(),
    }
}

/// The sentence the easy view opens with.
pub fn state_sentence(state: State, height: u64, language: Language) -> String {
    let next = height + 1;
    match (state, language) {
        (State::Updating(percent), Language::English) => format!("Updating {percent}%"),
        (State::Updating(percent), Language::Korean) => format!("업데이트 {percent}%"),
        (State::Syncing(target), Language::English) => format!("Sync {height}/{target}"),
        (State::Syncing(target), Language::Korean) => format!("{height}/{target} 받는 중"),
        (State::Mining, Language::English) => format!("Mining block {next}"),
        (State::Mining, Language::Korean) => format!("{next}번 블록 채굴 중"),
        (State::Idle, _) => language.pick("Mining is off", "채굴 꺼짐").into(),
        (State::Offline, _) => language.pick("Offline", "연결 끊김").into(),
        (State::MemoryFull, _) => language.pick("Memory is full", "메모리 부족").into(),
    }
}

fn reason_text(reason: ChainError, language: Language) -> &'static str {
    match reason {
        ChainError::BadLock => language.pick("wrong key", "서명 오류"),
        ChainError::MissingInput => language.pick("coins spent", "이미 쓴 코인"),
        ChainError::Overspend => language.pick("too much", "금액 초과"),
        ChainError::BadSeal | ChainError::BadDifficulty => language.pick("bad work", "작업 오류"),
        _ => language.pick("breaks rules", "규칙 위반"),
    }
}

fn event_text(event: Event, language: Language) -> String {
    let korean = language == Language::Korean;
    match event {
        Event::Mined { height } if korean => format!("{height}번 블록 채굴"),
        Event::Mined { height } => format!("mined block {height}"),
        Event::BlockAccepted { height } if korean => format!("{height}번 블록 받음"),
        Event::BlockAccepted { height } => format!("got block {height}"),
        Event::Reorganized { replaced, .. } if korean => format!("체인 교체 {replaced}블록"),
        Event::Reorganized { replaced, .. } => format!("chain replaced {replaced}"),
        Event::PaymentReceived { amount } if korean => format!("{} 받음", coins(amount)),
        Event::PaymentReceived { amount } => format!("received {}", coins(amount)),
        Event::PaymentSent { amount } if korean => format!("{} 보냄", coins(amount)),
        Event::PaymentSent { amount } => format!("sent {}", coins(amount)),
        Event::TxAccepted => language.pick("payment waiting", "송금 대기").into(),
        Event::TxReturned { .. } => language
            .pick("payment waiting again", "송금 다시 대기")
            .into(),
        Event::BlockRejected { reason } if korean => {
            format!("블록 거부 {}", reason_text(reason, language))
        }
        Event::BlockRejected { reason } => {
            format!("block: {}", reason_text(reason, language))
        }
        Event::TxRejected { reason } if korean => {
            format!("송금 거부 {}", reason_text(reason, language))
        }
        Event::TxRejected { reason } => {
            format!("payment: {}", reason_text(reason, language))
        }
        Event::TxDropped { .. } => language.pick("payment dropped", "송금 취소됨").into(),
        Event::NeedsHistory { .. } => language.pick("needs old blocks", "옛 블록 필요").into(),
        Event::ReorgRefused => language.pick("fork too deep", "분기가 너무 깊음").into(),
        Event::SnapshotAdopted { .. } => language.pick("snapshot taken", "스냅샷 받음").into(),
        Event::SnapshotRefused { .. } => language.pick("snapshot refused", "스냅샷 거부").into(),
        Event::DataRefused => language.pick("data too long", "데이터가 너무 김").into(),
        Event::CoinsMerged { inputs } if korean => format!("코인 {inputs}개 합침"),
        Event::CoinsMerged { inputs } => format!("merged {inputs} coins"),
    }
}

fn order_text(word: &'static str, language: Language) -> &'static str {
    match word {
        "limit" => language.pick(word, "상한"),
        "mine" => language.pick(word, "채굴"),
        "offline" => language.pick(word, "연결"),
        "pay" => language.pick(word, "송금"),
        "view" | "logo" => language.pick(word, "화면"),
        "lang" => language.pick(word, "언어"),
        "name" => language.pick(word, "이름"),
        _ => word,
    }
}

pub fn note_text(note: Note, language: Language) -> String {
    match note {
        Note::Chain(event) => event_text(event, language),
        Note::Order(word) => {
            let word = order_text(word, language);
            format!("{} {word}", language.pick("radio order", "원격 명령"))
        }
        Note::Updated(build) => format!("{} {build}", language.pick("updated to", "업데이트 완료")),
    }
}

pub fn uptime(now_ms: u64, language: Language) -> String {
    let minutes = now_ms / 60_000;
    match language {
        Language::English => format!("{}h{:02}m", minutes / 60, minutes % 60),
        Language::Korean => format!("{}시간 {}분", minutes / 60, minutes % 60),
    }
}

pub fn seconds(ms: u64, language: Language) -> String {
    format!("{}{}", ms / 1_000, language.pick("s", "초"))
}
