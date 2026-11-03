//! What a person does every day, in the order they look: their coins with Send and Receive and what
//! happened to them lately, this computer's mining with its speed, and the latest blocks with who
//! found each.
use super::coins::history_card;
use super::ledger::Ledger;
use super::mining_card::MiningCard;
use super::money::{ReceiveSheet, SendSheet};
use super::{Cx, Page};
use crate::core::unix_ms;
use crate::ui::chain::{self, Look, Strip};
use crate::ui::explain;
use crate::ui::floating::sheet;
use crate::ui::paint::ink;
use crate::ui::theme::desktop_reduces_motion;
use crate::ui::tokens;
use crate::ui::widgets::{self, button, card, dot, meta, section, Kind};
use crate::ui::{format, Icon, Tokens};
use egui::{vec2, Align, FontId, Label, Layout, RichText, Sense, Ui};

/// Home shows this many rows of what happened, and the Wallet the rest.
const RECENT_ROWS: usize = 4;
/// The blocks on Home are a little smaller than on Mining, so the rows below stay in sight.
const BLOCK_SIZE: f32 = 70.0;
/// How wide the coins take of the first row; mining takes the rest.
const COINS_SHARE: f32 = 0.42;

enum Open {
    Receive(ReceiveSheet),
    Send(SendSheet),
    Words(super::seed_words::WordsSheet),
}

#[derive(Default)]
pub struct Home {
    open: Option<Open>,
    ledger: Ledger,
    mining: MiningCard,
}

/// The large figure, which rolls to a new value over the length of a stamp.
fn balance_line(ui: &mut Ui, t: &Tokens, units: u64, ticker: &str, still: bool) {
    let id = ui.scope_id().with("balance");
    let target = units as f32;
    let seconds = if still { 0.0 } else { tokens::STAMP_SECONDS };
    let rolled = ui.ctx().animate_value_with_time(id, target, seconds);
    let shown = if (rolled - target).abs() < 1.0 { units } else { rolled as u64 };
    let figure = format::amount(shown);
    let font = FontId::new(tokens::AMOUNT, crate::ui::fonts::strong());
    let galley = ui.painter().layout_no_wrap(figure.clone(), font, t.text);
    let size = galley.size();
    let lit = ink(&galley, t.ink_top, t.ink_bottom);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
        ui.painter().galley(rect.min, lit, t.text);
        let said = format!("{figure} {ticker}");
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, said.as_str()));
        ui.with_layout(Layout::left_to_right(Align::Max), |ui| {
            ui.add_space(0.0);
            let text = RichText::new(ticker).size(tokens::TITLE).color(t.soft);
            ui.add(Label::new(text).selectable(false));
        });
    });
}

/// Says why mining cannot run or space ran out. True when the person wants to change that.
fn light_or_full(ui: &mut Ui, cx: &Cx) -> bool {
    let node = &cx.snap.node;
    let key = match () {
        () if node.light.is_some() => "home.light",
        () if node.storage_full => "home.full",
        () => return false,
    };
    ui.add_space(4.0);
    let open = ui
        .horizontal(|ui| {
            dot(ui, cx.t.warn);
            ui.add(Label::new(cx.say(key)).wrap());
            button(ui, cx.t, cx.say("home.open_settings"), Kind::Quiet).clicked()
        })
        .inner;
    light_limits(ui, cx);
    open
}

/// What a node without blocks cannot check, which decides how far its balance can be trusted.
fn light_limits(ui: &mut Ui, cx: &Cx) {
    let Some(light) = &cx.snap.node.light else {
        return;
    };
    if let Some(oldest) = light.pruned_from {
        ui.add_space(4.0);
        meta(ui, &cx.fill("home.light_pruned", &[&format::number(oldest.into())]), cx.t);
    }
    if light.unproven > 0 {
        ui.add_space(4.0);
        meta(ui, &cx.fill("home.light_unproven", &[&format::number(light.unproven as u128)]), cx.t);
    }
}

impl Home {
    /// Words not yet written down are the one way back to this wallet, so they ask here. They
    /// stand above what the node says, which comes and goes, so this button keeps its place.
    /// Words not yet written down come first; then a wallet with no words at all, which only
    /// wallets from before 0.2.0 are (new ones get words when made), so it shows whatever they
    /// hold: coins mined later need the words just the same; then coins the words cannot bring back.
    fn words_line(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        let node = &cx.snap.node;
        let words_line = match () {
            () if node.wallet.words_to_write.is_some() => Some((cx.say("home.words_unwritten").to_string(), "home.words_write")),
            // Before the core's first word the snapshot is empty, and an empty wallet has no words
            // either; the line waits for an address, so a wallet with words never flashes it.
            () if !node.wallet.has_words && !node.address.is_empty() => Some((cx.say("home.words_none").to_string(), "words.make")),
            () if node.wallet.outside_words > 0 => {
                Some((cx.fill("home.words_outside", &[&format::coins(node.wallet.outside_words, cx.ticker())]), "common.open"))
            }
            () => None,
        };
        let Some((line, open)) = words_line else {
            return;
        };
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            dot(ui, t.warn);
            // The text wraps in what the button leaves, so on a narrow window the button stays in sight.
            let label = cx.say(open);
            let room = ui.available_width() - widgets::button_width(ui, label) - ui.spacing().item_spacing.x;
            ui.scope(|ui| {
                ui.set_max_width(room.max(0.0));
                ui.add(Label::new(line).wrap());
            });
            if button(ui, t, label, Kind::Quiet).clicked() {
                self.open = Some(Open::Words(super::seed_words::WordsSheet::default()));
            }
        });
    }

    /// The coins: how many, the two things a person does with them, and what happened to them.
    fn coins(&mut self, ui: &mut Ui, cx: &mut Cx, still: bool, height: f32) {
        let t = cx.t;
        card(ui, t, |ui| {
            ui.set_min_height(height);
            let node = &cx.snap.node;
            meta(ui, cx.say("home.balance"), t);
            balance_line(ui, t, node.balance, cx.ticker(), still);
            // The line keeps its room, so the buttons stay put when the first block arrives.
            widgets::slot(ui, |ui| {
                ui.set_min_height(tokens::META_LINE);
                if node.maturing > 0 {
                    meta(ui, &cx.fill("home.soon", &[&format::coins(node.maturing, cx.ticker())]), t);
                }
            });
            ui.add_space(tokens::GAP);
            ui.horizontal(|ui| {
                if button(ui, t, cx.say("home.receive"), Kind::Main).clicked() {
                    self.open = Some(Open::Receive(ReceiveSheet::for_address(&node.address)));
                }
                if button(ui, t, cx.say("home.send"), Kind::Plain).clicked() {
                    self.open = Some(Open::Send(SendSheet::default()));
                }
            });
            self.words_line(ui, cx);
            if light_or_full(ui, cx) {
                *cx.go = Some("page.settings");
            }
            ui.add_space(tokens::GAP_PAGE);
            self.recent(ui, cx, still);
        });
    }

    /// Coins on the left and mining on the right, one height, or one above the other when narrow.
    fn first_row(&mut self, ui: &mut Ui, cx: &mut Cx, still: bool) {
        let heading = cx.say("page.mining");
        if ui.available_width() < tokens::COMPACT {
            self.coins(ui, cx, still, 0.0);
            ui.add_space(tokens::GAP_WIDE);
            self.mining.show(ui, cx, heading, "home", still);
            return;
        }
        let id = ui.scope_id().with("mining-height");
        let mining_height: f32 = ui.data(|data| data.get_temp(id)).unwrap_or(0.0);
        let total = ui.available_width();
        let left = ((total - tokens::GAP_WIDE) * COINS_SHARE).round();
        let right = total - left - tokens::GAP_WIDE;
        let margins = f32::from(tokens::CARD_MARGIN) * 2.0;
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(vec2(left, 0.0), Layout::top_down(Align::Min), |ui| {
                ui.set_width(left);
                self.coins(ui, cx, still, (mining_height - margins).max(0.0));
            });
            ui.add_space(tokens::GAP_WIDE - ui.spacing().item_spacing.x);
            let drawn = ui.allocate_ui_with_layout(vec2(right, 0.0), Layout::top_down(Align::Min), |ui| {
                ui.set_width(right);
                self.mining.show(ui, cx, heading, "home", still);
            });
            let height = drawn.response.rect.height();
            if (height - mining_height).abs() > 0.5 {
                ui.data_mut(|data| data.insert_temp(id, height));
                ui.ctx().request_repaint();
            }
        });
    }

    /// The latest blocks as boxes, with who found each, and a Details with what a block is.
    fn blocks(ui: &mut Ui, cx: &Cx, still: bool) {
        let t = cx.t;
        section(ui, cx.say("home.blocks"));
        explain::short(ui, t, cx.say("home.blocks_about"));
        ui.add_space(tokens::GAP);
        let ticker = cx.ticker().to_string();
        let look = Look { t, voice: cx.voice(), still, now_ms: unix_ms(), ticker: &ticker };
        let chain_view = &cx.snap.node.chain;
        chain::block_strip(ui, &look, &Strip::of(chain_view).size(BLOCK_SIZE));
        let labels = (cx.say("explain.details"), cx.say("explain.hide"));
        explain::details(ui, t, "home-blocks", labels, |ui| {
            explain::analogy(ui, t, cx.say("home.blocks_analogy"));
            ui.add_space(tokens::GAP);
            ui.label(cx.say("home.blocks_more"));
        });
    }

    /// The last few rows of what happened, and the way to all of them.
    fn recent(&mut self, ui: &mut Ui, cx: &mut Cx, still: bool) {
        let t = cx.t;
        ui.horizontal(|ui| {
            section(ui, cx.say("home.ledger"));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if !cx.snap.node.activity.is_empty() && button(ui, t, cx.say("home.see_all"), Kind::Quiet).clicked() {
                    *cx.go = Some("page.wallet");
                }
            });
        });
        ui.add_space(2.0);
        if !self.ledger.show(ui, cx, RECENT_ROWS, still) {
            // The button that starts mining stands above this, on the mining card.
            let body = cx.say(if cx.snap.node.mining { "home.empty_mining" } else { "home.empty_body" });
            meta(ui, body, t);
        }
    }

    fn sheets(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        let closed = match &mut self.open {
            None => false,
            Some(Open::Receive(receive)) => {
                let (done, outside) = sheet(ui, t, "receive", tokens::SLIP_WIDTH, |ui| receive.show(ui, cx));
                done || outside
            }
            Some(Open::Send(send)) => {
                let (done, outside) = sheet(ui, t, "send", tokens::SLIP_WIDTH, |ui| send.show(ui, cx));
                done || outside
            }
            Some(Open::Words(words)) => {
                let (done, outside) = sheet(ui, t, "words", tokens::SHEET_WIDTH, |ui| words.show(ui, cx));
                done || outside
            }
        };
        if closed {
            self.open = None;
        }
    }
}

impl Page for Home {
    fn title(&self) -> &'static str {
        "page.home"
    }

    fn icon(&self) -> Icon {
        Icon::Home
    }

    fn daily(&self) -> bool {
        true
    }

    fn show(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let still = cx.settings.reduce_motion || desktop_reduces_motion();
        self.first_row(ui, cx, still);
        ui.add_space(tokens::GAP_PAGE);
        // A person starts here, so what stops this node shows here.
        widgets::slot(ui, |ui| history_card(ui, cx));
        Self::blocks(ui, cx, still);
        self.sheets(ui, cx);
    }
}
