//! The two sheets that move money: one to receive with a code, one to send through a slip.
use super::Cx;
use crate::core::{parse_coins, BoardAction, BoardView, Command, LockOrder};
use crate::ui::floating::{sheet_head, Slip};
use crate::ui::tokens::{self, Tokens};
use crate::ui::widgets::{button, button_on, field, meta, mono, pick_row, Kind};
use crate::ui::format;
use egui::{vec2, Align, Color32, CornerRadius, Label, Layout, Rect, Sense, Ui};
use olc_control::BoardId;
use olc_core::CoinConfig;
use olc_service::qr::{qr_grid, QrGrid};
use olc_wallet::parse_address;
use olc_wallet::shared::{Shared, SHARED_START};

const CODE: f32 = 216.0;
const QUIET_SQUARES: f32 = 3.0;

pub struct ReceiveSheet {
    address: String,
    grid: Option<QrGrid>,
}

/// A phone camera reading a screen needs about four pixels a square.
const MIN_SQUARE: f32 = 4.0;

/// The size of one square, in points, of a code `width` squares wide: whole pixels, at least
/// [`MIN_SQUARE`] of them, and the code at least [`CODE`] points wide.
pub(super) fn code_square(width: usize, pixels_per_point: f32) -> f32 {
    let squares = width as f32 + QUIET_SQUARES * 2.0;
    let pixels = ((CODE / squares) * pixels_per_point).floor().max(MIN_SQUARE);
    pixels / pixels_per_point
}

/// Paints `grid` with its top left corner at `min`, `square` points a square.
fn paint_code(painter: &egui::Painter, min: egui::Pos2, grid: &QrGrid, square: f32) {
    let side = square * (grid.width as f32 + QUIET_SQUARES * 2.0);
    painter.rect_filled(Rect::from_min_size(min, vec2(side, side)), CornerRadius::same(tokens::RADIUS_CARD), Color32::WHITE);
    let start = min + vec2(square * QUIET_SQUARES, square * QUIET_SQUARES);
    for y in 0..grid.width {
        for x in 0..grid.width {
            if grid.is_dark(x, y) {
                let at = start + vec2(x as f32 * square, y as f32 * square);
                let cell = Rect::from_min_size(at, vec2(square + 0.3, square + 0.3));
                painter.rect_filled(cell, CornerRadius::ZERO, Color32::from_rgb(0x0E, 0x10, 0x14));
            }
        }
    }
}

/// Always dark on white, whatever the theme, so any camera can read it. Its size follows its
/// squares: at least four pixels each, in whole pixels so the edges stay sharp. One too big for
/// the room opens at full size on a click.
pub(super) fn code(ui: &mut Ui, grid: &QrGrid, name: &str) {
    let squares = grid.width as f32 + QUIET_SQUARES * 2.0;
    let wanted = code_square(grid.width, ui.ctx().pixels_per_point());
    let fits = wanted * squares <= ui.available_width();
    let square = if fits { wanted } else { ui.available_width() / squares };
    let side = square * squares;
    let sense = if fits { Sense::hover() } else { Sense::click() };
    let (rect, response) = ui.allocate_exact_size(vec2(side, side), sense);
    paint_code(ui.painter(), rect.min, grid, square);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, name));
    let open_id = response.id.with("large");
    if response.clicked() {
        ui.ctx().data_mut(|data| data.insert_temp(open_id, true));
    }
    let open = ui.ctx().data(|data| data.get_temp::<bool>(open_id)).unwrap_or(false);
    if open {
        let area = egui::Area::new(open_id).order(egui::Order::Foreground).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0));
        let shown = area.show(ui.ctx(), |ui| {
            let (rect, response) = ui.allocate_exact_size(vec2(wanted * squares, wanted * squares), Sense::click());
            paint_code(ui.painter(), rect.min, grid, wanted);
            response
        });
        if shown.inner.clicked() || ui.ctx().input(|input| input.key_pressed(egui::Key::Escape)) {
            ui.ctx().data_mut(|data| data.insert_temp(open_id, false));
        }
    }
}

impl ReceiveSheet {
    pub fn for_address(address: &str) -> Self {
        Self { address: address.to_string(), grid: qr_grid(address) }
    }

    /// True when it is finished.
    pub fn show(&mut self, ui: &mut Ui, cx: &Cx) -> bool {
        let t = cx.t;
        sheet_head(ui, cx.say("receive.title"));
        meta(ui, cx.say("receive.body"), t);
        ui.add_space(tokens::GAP);
        ui.vertical_centered(|ui| {
            if let Some(grid) = &self.grid {
                code(ui, grid, cx.say("receive.code"));
            }
        });
        ui.add_space(tokens::GAP);
        ui.add(Label::new(mono(self.address.clone())).wrap());
        ui.add_space(tokens::GAP);
        let mut done = false;
        ui.horizontal(|ui| {
            if button(ui, t, cx.say("receive.copy"), Kind::Main).clicked() {
                ui.ctx().copy_text(self.address.clone());
            }
            done = button(ui, t, cx.say("common.close"), Kind::Plain).clicked();
        });
        done
    }
}

/// A shared wallet's text, which can stand where an address does.
fn shared_wallet(text: &str) -> Option<Shared> {
    text.starts_with(SHARED_START).then(|| Shared::from_text(text).ok()).flatten()
}

/// Which wallet a slip pays from. `None` is this app's.
#[derive(Default)]
pub struct SendSheet {
    to: String,
    amount: String,
    tried: bool,
    payer: Option<BoardId>,
}

fn problem(ui: &mut Ui, t: &Tokens, text: &str) {
    ui.colored_label(t.bad, text);
}

/// A board that can hear an order to pay right now.
fn can_pay(board: &BoardView) -> bool {
    board.seen.online && !board.returning
}

/// The coin a board runs, when this app has heard of it. A fork keeps its coin's name and letters,
/// so the genesis is enough here.
pub(super) fn coin_of<'a>(cx: &Cx<'a>, board: &BoardView) -> Option<&'a CoinConfig> {
    let identities = cx.snap.groups.iter().filter_map(|group| group.identity.as_ref());
    let genesis = board.seen.beacon.genesis;
    identities.filter(|found| found.genesis == genesis).map(|found| &found.coin).next()
}

/// A board's balance in its own coin's letters.
pub(super) fn balance_of(cx: &Cx, board: &BoardView) -> String {
    let ticker = coin_of(cx, board).map_or_else(|| cx.ticker(), |coin| coin.ticker.as_str());
    format::coins(board.seen.beacon.balance, ticker)
}

fn wallet_row(ui: &mut Ui, cx: &Cx, name: &str, balance: &str, current: bool) -> bool {
    let row = pick_row(ui, cx.t, tokens::ROW, current, name, |ui| {
        ui.add(Label::new(name).truncate().selectable(false));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| meta(ui, balance, cx.t));
    });
    row.clicked()
}

impl SendSheet {
    /// The board paying, when one was picked and it can still hear.
    fn board<'a>(&self, cx: &Cx<'a>) -> Option<&'a BoardView> {
        let chosen = |board: &&BoardView| Some(board.seen.id) == self.payer && can_pay(board);
        cx.snap.boards.iter().find(chosen)
    }

    /// One wallet needs no choosing, so a person with no boards sees no list.
    fn wallet_rows(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        if cx.snap.boards.is_empty() {
            return;
        }
        meta(ui, cx.say("send.from"), t);
        let own = format::coins(cx.snap.node.spendable, cx.ticker());
        let on_board = self.board(cx).is_some();
        if wallet_row(ui, cx, cx.say("send.this_app"), &own, !on_board) {
            self.payer = None;
        }
        for board in &cx.snap.boards {
            let name = board.seen.beacon.name.as_str();
            if can_pay(board) {
                let current = self.board(cx).is_some_and(|paying| paying.seen.id == board.seen.id);
                if wallet_row(ui, cx, name, &balance_of(cx, board), current) {
                    self.payer = Some(board.seen.id);
                }
            } else {
                let line = cx.fill("send.off_board", &[name]);
                meta(ui, &line, t);
            }
        }
        ui.add_space(tokens::GAP);
    }

    fn slip(&self, cx: &Cx, units: u64, fee: u64) -> Slip {
        let ticker = cx.ticker();
        let to = self.to.trim().to_string();
        let board = self.board(cx);
        let have = board.map_or(cx.snap.node.spendable, |board| board.seen.beacon.balance);
        let left = have.saturating_sub(units + fee);
        let mut rows = Vec::new();
        if let Some(board) = board {
            rows.push((cx.say("send.from").to_string(), board.seen.beacon.name.clone()));
        }
        let shared = board.is_none().then(|| shared_wallet(&to)).flatten();
        let shown = match &shared {
            Some(shared) => cx.fill("send.to_shared", &[&shared.need.to_string(), &shared.keys.len().to_string()]),
            None => to.clone(),
        };
        rows.push((cx.say("send.to").to_string(), shown));
        rows.push((cx.say("send.fee").to_string(), format::coins(fee, ticker)));
        rows.push((cx.say("send.left").to_string(), format::coins(left, ticker)));
        let amount = self.amount.trim().to_string();
        let order = match board {
            Some(board) => {
                let action = BoardAction::Pay { to, amount: format::amount(units) };
                Command::Board { id: board.seen.id, action }
            }
            // A shared wallet's text stands for its lock, and coins go under it.
            None if shared.is_some() => Command::Lock(LockOrder::Pay { words: to, amount }),
            None => Command::Pay { to, amount },
        };
        Slip {
            what: cx.say("send.slip").to_string(),
            amount: Some(format::coins(units, ticker)),
            rows,
            warning: Some(cx.say("send.warning").to_string()),
            sign: cx.say("send.sign").to_string(),
            cancel: cx.say("common.back").to_string(),
            danger: false,
            order: vec![order],
        }
    }

    /// True when it is finished.
    pub fn show(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let t = cx.t;
        let node = &cx.snap.node;
        let ticker = cx.ticker();
        sheet_head(ui, cx.say("send.title"));
        let width = ui.available_width();
        self.wallet_rows(ui, cx);
        let board = self.board(cx);
        let coin = board.map_or(node.coin.as_ref(), |board| coin_of(cx, board));
        let next = board.map_or(node.height, |board| board.seen.beacon.height) + 1;
        let fee = coin.map_or(0, |coin| coin.rules_at(next).min_fee);
        let spendable = board.map_or(node.spendable, |board| board.seen.beacon.balance);
        meta(ui, &cx.fill("send.have", &[&format::coins(spendable, ticker)]), t);
        if spendable == 0 {
            meta(ui, cx.say("send.none"), t);
        }
        ui.add_space(4.0);
        field(ui, &mut self.to, cx.say("send.to_hint"), width);
        let shared = board.is_none() && shared_wallet(self.to.trim()).is_some();
        let address = parse_address(self.to.trim()).map(|_| ()).or(shared.then_some(()));
        if self.tried && address.is_none() {
            problem(ui, t, cx.say("send.bad_address"));
        }
        ui.horizontal(|ui| {
            let hint = cx.say("send.amount_hint");
            field(ui, &mut self.amount, hint, width - 100.0);
            if button(ui, t, cx.say("send.all"), Kind::Plain).clicked() {
                self.amount = format::amount(spendable.saturating_sub(fee));
            }
        });
        let units = parse_coins(self.amount.trim()).filter(|units| *units > 0);
        let enough = units.is_some_and(|units| units + fee <= spendable);
        if self.tried && units.is_none() {
            problem(ui, t, cx.say("send.bad_amount"));
        } else if self.tried && !enough {
            problem(ui, t, cx.say("send.too_much"));
        }
        ui.add_space(tokens::GAP);
        let mut done = false;
        ui.horizontal(|ui| {
            let filled = !self.to.trim().is_empty() && !self.amount.trim().is_empty();
            if button_on(ui, t, cx.say("send.next"), Kind::Main, filled).clicked() {
                self.tried = true;
                if let (Some(units), true, true) = (units, enough, address.is_some()) {
                    *cx.slip = Some(self.slip(cx, units, fee));
                    done = true;
                }
            }
            if button(ui, t, cx.say("common.cancel"), Kind::Plain).clicked() {
                done = true;
            }
        });
        done
    }
}

#[cfg(test)]
mod tests {
    use super::{code_square, MIN_SQUARE};
    use olc_service::qr::qr_grid;

    #[test]
    fn a_long_code_keeps_squares_a_camera_can_read() {
        // A two of three spend passed between signers is about 800 letters long.
        let spend = "a".repeat(796);
        let grid = qr_grid(&spend).unwrap();
        for pixels_per_point in [1.0, 1.5, 2.0] {
            let pixels = code_square(grid.width, pixels_per_point) * pixels_per_point;
            assert!(pixels >= MIN_SQUARE, "{pixels} pixels a square at {pixels_per_point}");
            assert_eq!(pixels, pixels.floor(), "whole pixels keep the edges sharp");
        }
        // A short address keeps its usual size.
        let address = qr_grid("olc409c2c2374917964bd7d010ddb77f90500eb3a41").unwrap();
        assert!(code_square(address.width, 1.0) * (address.width as f32 + 6.0) >= 200.0);
    }
}
