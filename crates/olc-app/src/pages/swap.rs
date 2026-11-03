//! Coin swaps with someone on another coin, with nobody in the middle. One side makes an offer,
//! the other answers it, and each passes one text. The nodes lock, take and give back by
//! themselves, and this page says where each swap stands in plain words.
use super::money::code;
use super::{Cx, Page};
use crate::core::swap::{Outcome, Stage};
use crate::core::swap_text::OfferText;
use crate::core::{Command, SwapOrder, SwapView};
use crate::ui::format;
use crate::ui::picker::{Picked, Picker};
use crate::ui::tokens;
use crate::ui::widgets::{button, button_on, card, field, meta, mono, section, segments, words_box, Kind};
use crate::ui::Icon;
use egui::{Label, Ui};
use olc_service::qr::qr_grid;

const WIDE: f32 = 560.0;
const AMOUNT: f32 = 160.0;
const COIN_ENDING: &str = "coin";
/// The waits a person picks from, in minutes, and the words for each.
const WAITS: [(u32, &str); 3] = [(10, "swap.wait_short"), (60, "swap.wait_hour"), (1_440, "swap.wait_day")];

/// The other coin of a new offer: one this app ran, by folder name, or a coin file.
#[derive(Clone, PartialEq, Eq)]
enum Other {
    Known(String),
    File(String),
}

pub struct Swap {
    other: Option<Other>,
    give: String,
    take: String,
    wait: u32,
    pasted: String,
    /// Answers pasted under each swap that waits for one, by swap id.
    answers: Vec<(String, String)>,
    /// The swap whose text shows as a code.
    code_of: Option<String>,
    picker: Option<Picker>,
    /// Whether the swaps that ended show, below the open ones.
    show_ended: bool,
}

impl Default for Swap {
    fn default() -> Self {
        Self {
            other: None,
            give: String::new(),
            take: String::new(),
            wait: WAITS[0].0,
            pasted: String::new(),
            answers: Vec::new(),
            code_of: None,
            picker: None,
            show_ended: false,
        }
    }
}

/// Where a swap stands, as a person reads it.
fn stage_key(swap: &SwapView) -> &'static str {
    match swap.stage {
        Stage::WaitingForAnswer => "swap.stage_answer",
        Stage::WaitingForTheirLock => "swap.stage_their_lock",
        Stage::Locked => "swap.stage_locked",
        Stage::Claiming => "swap.stage_claiming",
        Stage::Refunding => "swap.stage_refunding",
        Stage::Ended(Outcome::Swapped) => "swap.stage_done",
        Stage::Ended(Outcome::Refunded) => "swap.stage_refunded",
        Stage::Ended(Outcome::Cancelled) => "swap.stage_cancelled",
        Stage::Ended(Outcome::TooLate) => "swap.stage_too_late",
    }
}

/// A block on a coin, with about how long until it comes while it is still ahead.
fn block_at(cx: &Cx, ticker: &str, block: u64, (now, block_ms): (u64, u64)) -> String {
    let number = format::number(u128::from(block));
    let ahead = block.saturating_sub(now);
    if ahead == 0 {
        return cx.fill("swap.block", &[ticker, &number]);
    }
    let wait = format::span(cx.voice(), ahead.saturating_mul(block_ms) / 1_000);
    cx.fill("swap.block_ahead", &[ticker, &number, &wait])
}

impl Swap {
    fn other_ticker(&self, cx: &Cx) -> String {
        match &self.other {
            Some(Other::Known(name)) => {
                let known = cx.snap.swaps.coins.iter().find(|(folder, _)| folder == name);
                known.map_or_else(String::new, |(_, ticker)| ticker.clone())
            }
            Some(Other::File(path)) => path.rsplit(['/', '\\']).next().unwrap_or_default().to_string(),
            None => String::new(),
        }
    }

    fn one(&mut self, ui: &mut Ui, cx: &Cx, swap: &SwapView) {
        let t = cx.t;
        let (give, get) = (format::coins(swap.give.1, &swap.give.0), format::coins(swap.get.1, &swap.get.0));
        ui.label(cx.fill("swap.line", &[&give, &get]));
        meta(ui, cx.say(stage_key(swap)), t);
        if let (Stage::Locked, Some(from)) = (swap.stage, swap.refund_at) {
            let ticker = swap.give.0.as_str();
            let back = block_at(cx, ticker, from, (swap.height, swap.block_ms));
            let now = block_at(cx, ticker, swap.height, (swap.height, swap.block_ms));
            meta(ui, &cx.fill("swap.back_from", &[&back, &now]), t);
        }
        if cx.expert() {
            meta(ui, &cx.fill("pro.swap_id", &[&swap.id]), t);
        }
        if let Some(text) = &swap.text {
            ui.add_space(tokens::GAP);
            ui.add(Label::new(mono(text.clone())).wrap());
            ui.horizontal(|ui| {
                if button(ui, t, cx.say("swap.copy"), Kind::Plain).clicked() {
                    ui.ctx().copy_text(text.clone());
                }
                let showing = self.code_of.as_deref() == Some(swap.id.as_str());
                if button(ui, t, cx.say(if showing { "swap.hide_code" } else { "swap.show_code" }), Kind::Quiet).clicked() {
                    self.code_of = (!showing).then(|| swap.id.clone());
                }
            });
            if self.code_of.as_deref() == Some(swap.id.as_str()) {
                if let Some(grid) = qr_grid(&text.to_uppercase()) {
                    code(ui, &grid, cx.say("swap.code"));
                }
            }
        }
        if swap.stage == Stage::WaitingForAnswer {
            self.answer_row(ui, cx, &swap.id);
        }
        if matches!(swap.stage, Stage::WaitingForAnswer | Stage::WaitingForTheirLock)
            && button(ui, t, cx.say("swap.cancel"), Kind::Quiet).clicked()
        {
            cx.send(Command::Swap(SwapOrder::Cancel(swap.id.clone())));
        }
    }

    fn answer_row(&mut self, ui: &mut Ui, cx: &Cx, id: &str) {
        if !self.answers.iter().any(|(of, _)| of == id) {
            self.answers.push((id.to_string(), String::new()));
        }
        let Some((_, answer)) = self.answers.iter_mut().find(|(of, _)| of == id) else { return };
        ui.add_space(tokens::GAP);
        words_box(ui, answer, cx.say("swap.answer_hint"), WIDE);
        let ready = !answer.trim().is_empty();
        if button_on(ui, cx.t, cx.say("swap.start"), Kind::Main, ready).clicked() && ready {
            cx.send(Command::Swap(SwapOrder::Answer(answer.trim().to_string())));
            answer.clear();
        }
    }

    fn pick_coin(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let coins = &cx.snap.swaps.coins;
        let items: Vec<(usize, String)> = coins.iter().enumerate().map(|(at, (_, ticker))| (at, ticker.clone())).collect();
        // The first coin is chosen until another is, so the amount it asks for names a coin.
        if self.other.is_none() {
            self.other = coins.first().map(|(name, _)| Other::Known(name.clone()));
        }
        let current = coins.iter().position(|(name, _)| Some(&Other::Known(name.clone())) == self.other.as_ref());
        ui.horizontal(|ui| {
            if !items.is_empty() {
                if let Some(at) = segments(ui, cx.t, &items, current) {
                    self.other = coins.get(at).map(|(name, _)| Other::Known(name.clone()));
                }
            }
            if button(ui, cx.t, cx.say("swap.from_file"), Kind::Quiet).clicked() {
                self.picker = Some(Picker::open("swap.from_file", COIN_ENDING, cx.folder.as_deref()));
            }
        });
        if items.is_empty() && self.other.is_none() {
            meta(ui, cx.say("swap.no_coins"), cx.t);
        }
    }

    fn make(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        section(ui, cx.say("swap.make"));
        meta(ui, cx.say("swap.make_about"), t);
        ui.add_space(tokens::GAP);
        self.pick_coin(ui, cx);
        let other = self.other_ticker(cx);
        ui.add_space(tokens::GAP);
        // The names stay above the fields, since a hint goes away once a number is typed.
        let take = match other.is_empty() {
            true => cx.say("swap.take_any").to_string(),
            false => cx.fill("swap.take", &[&other]),
        };
        let give = cx.fill("swap.give", &[cx.ticker()]);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                meta(ui, &give, t);
                field(ui, &mut self.give, &give, AMOUNT);
            });
            ui.vertical(|ui| {
                meta(ui, &take, t);
                field(ui, &mut self.take, &take, AMOUNT);
            });
        });
        ui.add_space(tokens::GAP);
        meta(ui, cx.say("swap.wait"), t);
        let waits: Vec<(u32, String)> = WAITS.iter().map(|(minutes, key)| (*minutes, cx.say(key).to_string())).collect();
        if let Some(wait) = segments(ui, t, &waits, Some(self.wait)) {
            self.wait = wait;
        }
        let amounts = !self.give.trim().is_empty() && !self.take.trim().is_empty();
        let ready = amounts && self.other.is_some();
        ui.add_space(tokens::GAP);
        if button_on(ui, t, cx.say("swap.offer"), Kind::Main, ready).clicked() && ready {
            let coin = match &self.other {
                Some(Other::Known(name) | Other::File(name)) => name.clone(),
                None => return,
            };
            let (give, take) = (self.give.trim().to_string(), self.take.trim().to_string());
            cx.send(Command::Swap(SwapOrder::Offer { coin, give, take, wait_minutes: self.wait }));
            // A second press would make a second offer, so the form starts over.
            self.give.clear();
            self.take.clear();
        }
    }

    fn take_offer(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        section(ui, cx.say("swap.take_offer"));
        words_box(ui, &mut self.pasted, cx.say("swap.paste_hint"), WIDE);
        let offer = OfferText::from_text(&self.pasted);
        if let Some(offer) = &offer {
            let gives = format::coins(offer.take.units, &offer.take.ticker);
            let gets = format::coins(offer.give.units, &offer.give.ticker);
            meta(ui, &cx.fill("swap.offer_reads", &[&gives, &gets]), t);
        } else if !self.pasted.trim().is_empty() {
            meta(ui, cx.say("swap.bad_text"), t);
        }
        if button_on(ui, t, cx.say("swap.accept"), Kind::Main, offer.is_some()).clicked() && offer.is_some() {
            cx.send(Command::Swap(SwapOrder::Accept(self.pasted.trim().to_string())));
            self.pasted.clear();
        }
    }

    /// Swaps that finished, were cancelled or came back, folded away under one button.
    fn ended(&mut self, ui: &mut Ui, cx: &mut Cx, ended: &[SwapView]) {
        if ended.is_empty() {
            return;
        }
        ui.add_space(tokens::GAP);
        let count = ended.len().to_string();
        let words = match self.show_ended {
            true => cx.say("swap.ended_hide").to_string(),
            false => cx.fill("swap.ended_show", &[&count]),
        };
        if button(ui, cx.t, &words, Kind::Quiet).clicked() {
            self.show_ended = !self.show_ended;
        }
        if self.show_ended {
            for swap in ended {
                ui.add_space(tokens::GAP);
                card(ui, cx.t, |ui| self.one(ui, cx, swap));
            }
        }
    }

    fn file_sheet(&mut self, ui: &Ui, cx: &mut Cx) {
        let Some(picker) = &mut self.picker else { return };
        match picker.show(ui, cx.t, cx.voice()) {
            Picked::Waiting => return,
            Picked::Chosen(path) => {
                *cx.folder = path.parent().map(std::path::Path::to_path_buf);
                self.other = Some(Other::File(path.display().to_string()));
            }
            Picked::Cancelled => {}
        }
        self.picker = None;
    }
}

impl Page for Swap {
    fn title(&self) -> &'static str {
        "page.swap"
    }

    fn icon(&self) -> Icon {
        Icon::Swap
    }

    fn show(&mut self, ui: &mut Ui, cx: &mut Cx) {
        meta(ui, cx.say("swap.about"), cx.t);
        if let Some((ticker, height, peers)) = &cx.snap.swaps.other {
            let height = format::number(u128::from(*height));
            let key = if *peers == 0 { "swap.other_coin_alone" } else { "swap.other_coin" };
            meta(ui, &cx.fill(key, &[ticker, ticker, &height]), cx.t);
        }
        let (ended, open): (Vec<SwapView>, Vec<SwapView>) =
            cx.snap.swaps.swaps.iter().cloned().partition(|swap| matches!(swap.stage, Stage::Ended(_)));
        for swap in &open {
            ui.add_space(tokens::GAP);
            card(ui, cx.t, |ui| self.one(ui, cx, swap));
        }
        self.ended(ui, cx, &ended);
        ui.add_space(tokens::GAP_WIDE);
        self.make(ui, cx);
        ui.add_space(tokens::GAP_WIDE);
        self.take_offer(ui, cx);
        self.file_sheet(ui, cx);
    }
}
