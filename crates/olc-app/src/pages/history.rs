//! What happened to the money, row by row, as it happens: when (the clock and how long ago),
//! what kind, how sure it is now, and how much. A row opens into its whole detail.
//!
//! Two calls place it. [`HistoryList::recent`] draws the newest rows, for Home;
//! [`HistoryList::full`] draws every row kept with a kind filter and a CSV export, for the Wallet
//! page. Each page keeps its own `HistoryList` so an open detail stays with its page.
use super::history_detail::Detail;
use super::Cx;
use crate::core::{unix_ms, Activity, ActivityKind, Command, NodeView, Personal, KEPT};
use crate::i18n::{self, Voice};
use crate::ui::explain;
use crate::ui::floating::sheet;
use crate::ui::picker::{Picked, Picker};
use crate::ui::theme::desktop_reduces_motion;
use crate::ui::tokens;
use crate::ui::widgets::{self, button, header_cell, meta, pick_row, segments, Kind};
use crate::ui::{format, Tokens};
use egui::{vec2, Align, Color32, CornerRadius, Label, Layout, Rect, RichText, Sense, TextStyle, TextWrapMode, Ui, WidgetText};
use olc_core::Hash;

/// Blocks on top of a payment before a basic row calls it settled, as Bitcoin wallets count six.
pub const SETTLED: u64 = 6;
/// The columns' shares when the window is too narrow for every text whole.
const WEIGHTS: [f32; 4] = [1.25, 1.9, 1.1, 1.0];
/// What happened keeps at least this share of a row, room for a kind's name such as "Received";
/// the rest go to whole texts first.
const LEAST_WHAT: f32 = 0.14;
/// The clock keeps at least this share before the weights take over.
const LEAST_WHEN: f32 = 0.12;
/// Column widths step by this much, so a growing seconds count does not shift them.
const WIDTH_STEP: f32 = 8.0;
/// How wide the bar to settled is.
const SETTLE_BAR: f32 = 36.0;
/// Rows drawn at once on the full list; the rest one press away, so a long history stays quick.
const PAGE_ROWS: usize = 200;

/// Which kinds the full list shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Filter {
    #[default]
    All,
    Only(ActivityKind),
}

impl Filter {
    fn keeps(self, item: &Activity) -> bool {
        match self {
            Filter::All => true,
            Filter::Only(kind) => item.kind == kind,
        }
    }
}

#[derive(Default)]
pub struct HistoryList {
    filter: Filter,
    shown: usize,
    detail: Option<Detail>,
    saving: Option<Picker>,
    saved: Option<String>,
    newest: Option<Hash>,
    arrived_at: f64,
}

pub fn kind_key(kind: ActivityKind) -> &'static str {
    match kind {
        ActivityKind::Mined => "history.mined",
        ActivityKind::Received => "history.received",
        ActivityKind::Sent => "history.sent",
        ActivityKind::ToSelf => "history.to_self",
        ActivityKind::Joined => "history.joined",
    }
}

/// How a row changes the balance: plus, minus, or only its fee for coins that stay.
pub fn signed_amount(item: &Activity, ticker: &str) -> String {
    match item.kind {
        ActivityKind::Mined | ActivityKind::Received => format!("+{}", format::coins(item.amount, ticker)),
        ActivityKind::Sent => format!("-{}", format::coins(item.amount + item.fee.unwrap_or(0), ticker)),
        ActivityKind::ToSelf | ActivityKind::Joined => match item.fee {
            Some(fee) if fee > 0 => format!("-{}", format::coins(fee, ticker)),
            _ => format::coins(0, ticker),
        },
    }
}

/// Every row there is: the wallet's waiting payments, then each settled one, newest first.
pub fn every_row(node: &NodeView) -> impl Iterator<Item = &Activity> {
    let waiting = node.activity.iter().filter(|item| item.height.is_none());
    let settled = node.wallet.history.iter();
    // Before the first full list arrives, the short one stands in.
    let early = node.wallet.history.is_empty().then(|| node.activity.iter().filter(|item| item.height.is_some()));
    waiting.chain(settled).chain(early.into_iter().flatten())
}

/// Blocks on top of a row's block, counting its own. `None` while it waits.
pub fn confirmations(item: &Activity, tip: u64) -> Option<u64> {
    item.height.map(|height| (tip + 1).saturating_sub(height))
}

/// Blocks until mined coins can be spent. `None` once they can, or for any other row.
pub fn blocks_to_ripe(item: &Activity, tip: u64) -> Option<u64> {
    item.ripe_at.map(|ripe| ripe.saturating_sub(tip + 1)).filter(|left| *left > 0)
}

pub fn label_of<'a>(cx: &'a Cx, id: &Hash) -> Option<&'a str> {
    let key: String = id.iter().map(|byte| format!("{byte:02x}")).collect();
    cx.snap.node.wallet.labels.get(&key).map(String::as_str)
}

/// The words of the "what" cell: the kind, a join's count and fee, and a name if it has one.
pub fn what_line(cx: &Cx, item: &Activity) -> String {
    labelled(cx, item, what_words(cx.voice(), cx.ticker(), item, false))
}

/// What happened, whole where `width` holds it. Narrower, the amounts are left to the detail, so
/// the cut, if any, never falls inside a number.
fn what_fitting(ui: &Ui, cx: &Cx, item: &Activity, width: f32) -> String {
    let whole = what_line(cx, item);
    if natural(ui, whole.as_str()) <= width {
        return whole;
    }
    labelled(cx, item, what_words(cx.voice(), cx.ticker(), item, true))
}

fn labelled(cx: &Cx, item: &Activity, what: String) -> String {
    match label_of(cx, &item.id) {
        Some(label) => format!("{what} · {label}"),
        None => what,
    }
}

/// What happened in words, its amounts written as every amount is. `short` leaves them out.
fn what_words(voice: Voice, ticker: &str, item: &Activity, short: bool) -> String {
    let say = |key| i18n::text(voice, key);
    let count = format::number(u128::try_from(item.coins_in).unwrap_or(u128::MAX));
    match item.kind {
        ActivityKind::Joined if short => i18n::fill(say("history.joined_short"), &[&count]),
        // The amount column is the change to the balance, so the words say what moved.
        ActivityKind::Joined => {
            let fee = item.fee.unwrap_or(0);
            let total = format::coins(item.amount.saturating_add(fee), ticker);
            i18n::fill(say("history.joined_line"), &[&count, &total, &format::coins(fee, ticker)])
        }
        ActivityKind::ToSelf if !short => i18n::fill(say("history.to_self_line"), &[&format::coins(item.amount, ticker)]),
        kind => say(kind_key(kind)).to_string(),
    }
}

/// A small bar that fills as blocks pile on, to "settled" at [`SETTLED`].
fn settle_bar(ui: &mut Ui, t: &Tokens, filled: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(SETTLE_BAR, 6.0), Sense::hover());
    let round = CornerRadius::same(3);
    ui.painter().rect_filled(rect, round, t.line_strong);
    let part = Rect::from_min_size(rect.min, vec2(rect.width() * filled.clamp(0.0, 1.0), rect.height()));
    let colour = if filled >= 1.0 { t.good } else { t.info };
    ui.painter().rect_filled(part, round, colour);
}

/// How sure a row is now, as its status cell says it.
enum Status {
    /// Waiting for a block, in a small tag.
    Waiting(String),
    /// Words in a colour: blocks until mined coins ripen, or confirmations.
    Words(String, Color32),
    /// A bar filling to settled, with its word.
    Settling(f32, String),
}

fn status(cx: &Cx, item: &Activity) -> Status {
    let t = cx.t;
    let tip = cx.snap.node.height;
    if item.height.is_none() {
        return Status::Waiting(cx.say("history.waiting").to_string());
    }
    if let Some(left) = blocks_to_ripe(item, tip) {
        return Status::Words(cx.fill("history.ripe_in", &[&format::number(left.into())]), t.warn);
    }
    let count = confirmations(item, tip).unwrap_or(0);
    if cx.expert() {
        return Status::Words(cx.fill("history.confirmations", &[&format::number(count.into())]), t.soft);
    }
    let key = if count >= SETTLED { "history.settled" } else { "history.confirming" };
    // A fraction of a bar: six blocks at most, nothing lost in the float.
    Status::Settling(count as f32 / SETTLED as f32, cx.say(key).to_string())
}

/// How sure a row is now: waiting, blocks until mined coins ripen, or confirmations.
pub fn status_cell(ui: &mut Ui, cx: &Cx, item: &Activity) {
    let t = cx.t;
    match status(cx, item) {
        Status::Waiting(word) => {
            widgets::tag(ui, &word, t.warn, t);
        }
        Status::Words(words, colour) => {
            ui.add(Label::new(RichText::new(words).color(colour)).truncate());
        }
        Status::Settling(filled, word) => {
            settle_bar(ui, t, filled);
            ui.add(Label::new(RichText::new(word).color(t.soft)).truncate());
        }
    }
}

/// How wide a text stands on one line, as a label draws it.
fn natural(ui: &Ui, text: impl Into<WidgetText>) -> f32 {
    text.into().into_galley(ui, Some(TextWrapMode::Extend), f32::INFINITY, TextStyle::Body).size().x
}

/// How wide [`status_cell`] draws a row's status whole.
fn status_width(ui: &Ui, cx: &Cx, item: &Activity) -> f32 {
    match status(cx, item) {
        Status::Waiting(word) => natural(ui, RichText::new(word).size(tokens::META)) + widgets::TAG_PAD,
        Status::Words(words, _) => natural(ui, words),
        Status::Settling(_, word) => SETTLE_BAR + ui.spacing().item_spacing.x + natural(ui, word),
    }
}

/// The columns' widths for these rows in `width`, and whether the time column holds the clock
/// alone. When, status and amount are as wide as their widest text, so none is cut; what happened
/// takes the rest and is cut first, since its whole line is in the detail. Too narrow for that,
/// the time drops how long ago and keeps its clock whole; narrower still, the weights share.
fn widths(ui: &Ui, cx: &Cx, items: &[&Activity], width: f32) -> ([f32; 4], bool) {
    let gap = ui.spacing().item_spacing.x;
    let room = (width - gap * 3.0).max(1.0);
    let now = unix_ms();
    let ticker = cx.ticker();
    let heading = |key: &str| natural(ui, RichText::new(cx.say(key)).size(tokens::META));
    let widest = |key: &str, each: &dyn Fn(&Activity) -> f32| {
        // Stepped up, so a seconds count that grows a digit does not shift the columns.
        let most = items.iter().map(|item| each(item)).fold(heading(key), f32::max);
        (most / WIDTH_STEP).ceil() * WIDTH_STEP
    };
    let status = widest("history.status", &|item| status_width(ui, cx, item));
    let amount = widest("history.amount", &|item| natural(ui, widgets::medium_text(signed_amount(item, ticker))));
    let left = room - room * LEAST_WHAT - status - amount;
    let when = widest("history.when", &|item| natural(ui, format::when(cx.voice(), now, item.time_ms)));
    let clock = widest("history.when", &|item| natural(ui, format::clock(now, item.time_ms)));
    let (when, clock_only) = match () {
        () if when <= left => (when, false),
        () if clock <= left => (clock, true),
        () if left >= room * LEAST_WHEN => (left, true),
        () => {
            let total: f32 = WEIGHTS.iter().sum();
            return (WEIGHTS.map(|weight| weight / total * room), false);
        }
    };
    ([when, room - when - status - amount, status, amount], clock_only)
}

impl HistoryList {
    /// The key of the title a page holding [`HistoryList::full`] puts above it.
    pub const TITLE: &'static str = "history.heading";

    /// The newest `rows` rows, for Home. A row opens its detail.
    pub fn recent(&mut self, ui: &mut Ui, cx: &mut Cx, rows: usize) {
        let snap = cx.snap;
        let items: Vec<&Activity> = every_row(&snap.node).take(rows).collect();
        if items.is_empty() {
            let body = cx.say(if cx.snap.node.mining { "history.empty_mining" } else { "history.empty_body" });
            widgets::empty(ui, cx.t, cx.say("history.empty"), body, None);
            return;
        }
        self.rows(ui, cx, &items);
        self.sheets(ui, cx);
    }

    /// Every row kept, with a kind filter and a CSV export, for the Wallet page.
    pub fn full(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        // Wrapped, so a long language moves the button down instead of widening the column.
        ui.horizontal_wrapped(|ui| {
            let mut kinds = vec![(Filter::All, cx.say("history.all").to_string())];
            for kind in [ActivityKind::Received, ActivityKind::Sent, ActivityKind::Mined, ActivityKind::Joined, ActivityKind::ToSelf] {
                kinds.push((Filter::Only(kind), cx.say(kind_key(kind)).to_string()));
            }
            if let Some(chosen) = segments(ui, t, &kinds, Some(self.filter)) {
                self.filter = chosen;
                self.shown = 0;
            }
            if button(ui, t, cx.say("history.export"), Kind::Plain).clicked() {
                self.saving = Some(Picker::save("history.export", "history", "csv", cx.folder.as_deref()));
            }
        });
        if let Some(saved) = &self.saved {
            meta(ui, saved, t);
        }
        // What each kind means stands under the kinds it explains, above the rows, so a new row
        // never moves it.
        explain::details(ui, t, "history-kinds", (cx.say("explain.details"), cx.say("explain.hide")), |ui| {
            super::history_detail::kinds_explained(ui, cx);
        });
        ui.add_space(tokens::GAP);
        let shown = self.shown.max(PAGE_ROWS);
        let snap = cx.snap;
        let kept: Vec<&Activity> = every_row(&snap.node).filter(|item| self.filter.keeps(item)).collect();
        if kept.is_empty() {
            meta(ui, cx.say("history.none_of_kind"), t);
        } else {
            self.rows(ui, cx, &kept[..kept.len().min(shown)]);
        }
        if kept.len() > shown && button(ui, t, cx.say("history.more"), Kind::Quiet).clicked() {
            self.shown = shown + PAGE_ROWS;
        }
        meta(ui, &cx.fill("history.kept", &[&format::number(KEPT as u128)]), t);
        self.save_csv(ui, cx);
        self.sheets(ui, cx);
    }

    fn rows(&mut self, ui: &mut Ui, cx: &mut Cx, items: &[&Activity]) {
        let t = cx.t;
        let still = cx.settings.reduce_motion || desktop_reduces_motion();
        let now_time = ui.input(|input| input.time);
        let first = items.first().map(|item| item.id);
        if first != self.newest {
            if self.newest.is_some() {
                self.arrived_at = now_time;
            }
            self.newest = first;
        }
        let arrived = ((now_time - self.arrived_at) as f32 / tokens::STAMP_SECONDS).clamp(0.0, 1.0);
        let (cells, clock_only) = widths(ui, cx, items, ui.available_width());
        let columns = widgets::Columns::sized(&cells);
        columns.row(ui, |ui, index| {
            let key = ["history.when", "history.what", "history.status", "history.amount"][index];
            match index {
                3 => ui.with_layout(Layout::right_to_left(Align::Center), |ui| header_cell(ui, t, cx.say(key))).inner,
                _ => header_cell(ui, t, cx.say(key)),
            }
        });
        widgets::divider(ui, t);
        let now = unix_ms();
        let ticker = cx.ticker().to_string();
        let voice = cx.voice();
        let mut opened = None;
        for (at, item) in items.iter().enumerate() {
            // A new row slides in from the left and its wash fades, unless motion is reduced.
            let sliding = at == 0 && arrived < 1.0 && !still;
            let shift = if sliding { (1.0 - arrived) * 28.0 } else { 0.0 };
            let name = format!("{} {}", what_line(cx, item), signed_amount(item, &ticker));
            let row = pick_row(ui, t, tokens::ROW, false, &name, |ui| {
                ui.add_space(shift);
                // A row sliding in gives its shift from what happened, so the rest stay whole.
                let mut cells = cells;
                cells[1] = (cells[1] - shift).max(0.0);
                let size = |at: usize| vec2(cells[at], tokens::ROW);
                // Each cell keeps its column's width whatever it holds, so the columns line up.
                ui.allocate_ui_with_layout(size(0), Layout::left_to_right(Align::Center), |ui| {
                    ui.set_width(cells[0]);
                    let when = if clock_only { format::clock(now, item.time_ms) } else { format::when(voice, now, item.time_ms) };
                    let text = RichText::new(when).color(t.soft);
                    ui.add(Label::new(text).truncate().selectable(false));
                });
                ui.allocate_ui_with_layout(size(1), Layout::left_to_right(Align::Center), |ui| {
                    ui.set_width(cells[1]);
                    ui.add(Label::new(what_fitting(ui, cx, item, cells[1])).truncate().selectable(false));
                });
                ui.allocate_ui_with_layout(size(2), Layout::left_to_right(Align::Center), |ui| {
                    ui.set_width(cells[2]);
                    status_cell(ui, cx, item);
                });
                ui.allocate_ui_with_layout(size(3), Layout::right_to_left(Align::Center), |ui| {
                    ui.set_width(cells[3]);
                    ui.add(Label::new(widgets::medium_text(signed_amount(item, &ticker))).truncate().selectable(false));
                });
            });
            if sliding {
                let wash = t.good.gamma_multiply(0.14 * (1.0 - arrived));
                ui.painter().rect_filled(row.rect, CornerRadius::same(tokens::RADIUS_BUTTON), wash);
                ui.ctx().request_repaint();
            }
            if row.clicked() {
                opened = Some((*item).clone());
            }
        }
        // The clock and the seconds keep counting.
        ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
        if let Some(item) = opened {
            // Its block holds it with every coin in and out, mined rows too.
            if let Some(height) = item.height {
                cx.send(Command::Own(Personal::Explore(height.to_string())));
            }
            self.detail = Some(Detail::new(item));
        }
    }

    fn sheets(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let Some(detail) = &mut self.detail else {
            return;
        };
        let (done, outside) = sheet(ui, cx.t, "history-detail", tokens::SHEET_WIDE, |ui| detail.show(ui, cx));
        if done || outside {
            self.detail = None;
        }
    }

    fn save_csv(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let Some(picker) = &mut self.saving else {
            return;
        };
        match picker.show(ui, cx.t, cx.voice()) {
            Picked::Waiting => {}
            Picked::Cancelled => self.saving = None,
            Picked::Chosen(path) => {
                self.saving = None;
                *cx.folder = path.parent().map(std::path::Path::to_path_buf);
                let snap = cx.snap;
                let rows: Vec<&Activity> = every_row(&snap.node).filter(|item| self.filter.keeps(item)).collect();
                let text = csv(cx, &rows);
                let shown = path.display().to_string();
                self.saved = Some(match std::fs::write(&path, text) {
                    Ok(()) => cx.fill("history.saved", &[&shown]),
                    Err(error) => cx.fill("history.save_failed", &[&error.to_string()]),
                });
            }
        }
    }
}

/// A field of a CSV line, quoted when it holds a comma, a quote or a line break.
fn field(text: &str) -> String {
    match text.contains([',', '"', '\n']) {
        true => format!("\"{}\"", text.replace('"', "\"\"")),
        false => text.to_string(),
    }
}

/// The rows as a spreadsheet reads them, in English column names as Bitcoin Core writes them.
pub fn csv(cx: &Cx, rows: &[&Activity]) -> String {
    let mut out = String::from("Date,Kind,Amount,Fee,Confirmations,Block,ID,Label\n");
    let tip = cx.snap.node.height;
    for item in rows {
        let id: String = item.id.iter().map(|byte| format!("{byte:02x}")).collect();
        let fields = [
            format::stamp(item.time_ms),
            format!("{:?}", item.kind),
            format::amount(item.amount),
            item.fee.map(format::amount).unwrap_or_default(),
            confirmations(item, tip).map(|count| count.to_string()).unwrap_or_default(),
            item.height.map(|height| height.to_string()).unwrap_or_default(),
            id,
            label_of(cx, &item.id).unwrap_or_default().to_string(),
        ];
        let line: Vec<String> = fields.iter().map(|text| field(text)).collect();
        out.push_str(&line.join(","));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{Lang, Mode};

    fn row(kind: ActivityKind, height: Option<u64>, ripe_at: Option<u64>) -> Activity {
        Activity { kind, height, amount: 400, time_ms: 0, id: [1; 32], fee: Some(1), size: 900, coins_in: 8, ripe_at }
    }

    #[test]
    fn confirmations_count_up_and_mined_coins_say_when_they_ripen() {
        let mined = row(ActivityKind::Mined, Some(10), Some(13));
        assert_eq!(confirmations(&mined, 10), Some(1));
        assert_eq!(confirmations(&mined, 15), Some(6));
        assert_eq!(blocks_to_ripe(&mined, 10), Some(2), "spendable in the block at 13, two blocks after the next");
        assert_eq!(blocks_to_ripe(&mined, 12), None);
        assert_eq!(confirmations(&row(ActivityKind::Sent, None, None), 10), None);
    }

    /// A join's words carry its amounts as every amount is written, and its short form none.
    #[test]
    fn a_join_says_its_amounts_as_amounts_are_written() {
        let mut joined = row(ActivityKind::Joined, Some(1), None);
        (joined.coins_in, joined.amount, joined.fee) = (1_600, 1_999_999_000, Some(1_000));
        let ko = Voice { lang: Lang::Ko, mode: Mode::Basic };
        assert_eq!(what_words(ko, "OLC", &joined, false), "코인 1,600개(20 OLC)를 하나로 합침, 수수료 0.00001 OLC");
        assert_eq!(what_words(ko, "OLC", &joined, true), "코인 1,600개를 하나로 합침");
        let en = Voice { lang: Lang::En, mode: Mode::Expert };
        assert_eq!(what_words(en, "OLC", &joined, true), "1,600 coins joined into one");
    }

    #[test]
    fn coins_that_stay_lose_only_their_fee() {
        assert_eq!(signed_amount(&row(ActivityKind::Joined, Some(1), None), "OLC"), format!("-{}", format::coins(1, "OLC")));
        assert_eq!(signed_amount(&row(ActivityKind::Sent, Some(1), None), "OLC"), format!("-{}", format::coins(401, "OLC")));
        assert!(signed_amount(&row(ActivityKind::Mined, Some(1), None), "OLC").starts_with('+'));
        assert_eq!(field("a,b"), "\"a,b\"");
        assert_eq!(field("say \"hi\""), "\"say \"\"hi\"\"\"");
    }
}
