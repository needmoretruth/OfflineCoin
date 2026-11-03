//! The two sheets that move money: one to receive with a code, one to send through a slip.
use super::Cx;
use crate::core::{parse_coins, BoardView, Command, Personal, WalletOrder};
use crate::ui::explain;
use crate::ui::floating::sheet_head;
use crate::ui::tokens::{self, Tokens};
use crate::ui::widgets::{button, meta, mono, pick_row, Kind};
use crate::ui::format;
use egui::{vec2, Align, Color32, CornerRadius, Label, Layout, Rect, Sense, Ui};
use olc_core::CoinConfig;
use olc_service::qr::{qr_grid, QrGrid};
use olc_wallet::parse_address;
use olc_wallet::shared::{Shared, SHARED_START};

const CODE: f32 = 216.0;
const QUIET_SQUARES: f32 = 3.0;

pub struct ReceiveSheet {
    /// The address of a board, which stays as it was given. `None` follows this wallet's
    /// receiving address, which "New address" changes.
    fixed: Option<String>,
    amount: String,
    note: String,
    /// What the code holds, and the code.
    shown: String,
    grid: Option<QrGrid>,
}

/// The scheme of a payment request, as BIP 21's `bitcoin:` is Bitcoin's.
pub const REQUEST_SCHEME: &str = "olc:";

/// Percent-encodes what a request's query may not hold as it is (RFC 3986 unreserved stays).
fn escape(text: &str) -> String {
    let keep = |byte: u8| byte.is_ascii_alphanumeric() || b"-._~".contains(&byte);
    text.bytes().map(|byte| if keep(byte) { (byte as char).to_string() } else { format!("%{byte:02X}") }).collect()
}

fn unescape(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let pair = text.get(at + 1..at + 3).and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match (bytes[at], pair) {
            (b'%', Some(byte)) => {
                out.push(byte);
                at += 3;
            }
            (b'+', _) => {
                out.push(b' ');
                at += 1;
            }
            (byte, _) => {
                out.push(byte);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A request to pay `address`, as BIP 21 shapes one: `olc:ADDRESS?amount=1.5&message=Lunch`.
/// With no amount and no note it is the bare address, which every wallet reads.
pub fn request_uri(address: &str, amount: Option<u64>, note: &str) -> String {
    let mut query = Vec::new();
    if let Some(units) = amount.filter(|units| *units > 0) {
        query.push(format!("amount={}", format::amount(units).replace(',', "")));
    }
    if !note.trim().is_empty() {
        query.push(format!("message={}", escape(note.trim())));
    }
    match query.is_empty() {
        true => address.to_string(),
        false => format!("{REQUEST_SCHEME}{address}?{}", query.join("&")),
    }
}

/// The address, amount and note of a request, when `text` is one.
pub fn parse_request(text: &str) -> Option<(String, Option<u64>, String)> {
    let rest = text.trim().strip_prefix(REQUEST_SCHEME)?;
    let (address, query) = rest.split_once('?').unwrap_or((rest, ""));
    parse_address(address)?;
    let (mut amount, mut note) = (None, String::new());
    for pair in query.split('&') {
        match pair.split_once('=') {
            Some(("amount", value)) => amount = parse_coins(value),
            Some(("message" | "label", value)) if note.is_empty() => note = unescape(value),
            _ => {}
        }
    }
    Some((address.to_string(), amount, note))
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
    /// A sheet for an address. Given this wallet's receiving address, it follows that address,
    /// so "New address" shows the new one; a board's address stays as given.
    pub fn for_address(address: &str) -> Self {
        Self { fixed: Some(address.to_string()), amount: String::new(), note: String::new(), shown: String::new(), grid: None }
    }

    /// True when it is finished.
    pub fn show(&mut self, ui: &mut Ui, cx: &Cx) -> bool {
        let t = cx.t;
        // This wallet's own address follows its receiving key from then on.
        if self.fixed.as_deref() == Some(cx.snap.node.address.as_str()) {
            self.fixed = None;
        }
        let address = self.fixed.clone().unwrap_or_else(|| cx.snap.node.address.clone());
        let amount = parse_coins(self.amount.trim()).filter(|units| *units > 0);
        let wanted = request_uri(&address, amount, &self.note);
        if wanted != self.shown || self.grid.is_none() {
            self.grid = qr_grid(&wanted);
            self.shown = wanted;
        }
        sheet_head(ui, cx.say("receive.title"));
        meta(ui, cx.say("receive.body"), t);
        ui.add_space(tokens::GAP);
        ui.vertical_centered(|ui| {
            if let Some(grid) = &self.grid {
                code(ui, grid, cx.say("receive.code"));
            }
        });
        ui.add_space(tokens::GAP);
        ui.add(Label::new(mono(address.clone())).wrap());
        if let Some(label) = cx.snap.node.wallet.labels.get(&address) {
            meta(ui, label, t);
        }
        ui.add_space(tokens::GAP);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| named_field(ui, t, cx.say("receive.amount_hint"), &mut self.amount, "", 150.0));
            ui.vertical(|ui| named_field(ui, t, cx.say("receive.note_hint"), &mut self.note, "", ui.available_width()));
        });
        if !self.amount.trim().is_empty() && amount.is_none() {
            problem(ui, t, cx.say("send.bad_amount"));
        }
        explain::short(ui, t, cx.say("receive.request_about"));
        if cx.expert() && self.shown != address {
            ui.add(Label::new(mono(self.shown.clone())).wrap());
        }
        ui.add_space(tokens::GAP);
        let mut done = false;
        ui.horizontal(|ui| {
            let copy = if self.shown == address { "receive.copy" } else { "receive.copy_request" };
            if button(ui, t, cx.say(copy), Kind::Main).clicked() {
                ui.ctx().copy_text(self.shown.clone());
            }
            if self.fixed.is_none() && button(ui, t, cx.say("receive.new_address"), Kind::Plain).clicked() {
                cx.send(Command::Own(Personal::Wallet(WalletOrder::NewAddress)));
            }
            done = button(ui, t, cx.say("common.close"), Kind::Plain).clicked();
        });
        done
    }
}

pub use super::send::SendSheet;

/// A shared wallet's text, which can stand where an address does.
pub(super) fn shared_wallet(text: &str) -> Option<Shared> {
    text.starts_with(SHARED_START).then(|| Shared::from_text(text).ok()).flatten()
}

/// A field whose name stays above it after typing, with an example inside while it is empty.
pub(super) fn named_field(ui: &mut Ui, t: &Tokens, name: &str, text: &mut String, example: &str, width: f32) -> egui::Response {
    meta(ui, name, t);
    let edit = egui::TextEdit::singleline(text)
        .hint_text(example)
        .desired_width(width)
        .margin(vec2(10.0, 8.0))
        .align(egui::Align2::LEFT_CENTER)
        .min_size(vec2(width, tokens::CONTROL));
    ui.add(edit).accessible_name(name)
}

pub(super) fn problem(ui: &mut Ui, t: &Tokens, text: &str) {
    ui.colored_label(t.bad, text);
}

/// A board that can hear an order to pay right now.
pub(super) fn can_pay(board: &BoardView) -> bool {
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

pub(super) fn wallet_row(ui: &mut Ui, cx: &Cx, name: &str, balance: &str, current: bool) -> bool {
    let row = pick_row(ui, cx.t, tokens::ROW, current, name, |ui| {
        ui.add(Label::new(name).truncate().selectable(false));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| meta(ui, balance, cx.t));
    });
    row.clicked()
}

#[cfg(test)]
mod tests {
    use super::{code_square, parse_request, request_uri, MIN_SQUARE};

    #[test]
    fn a_request_carries_its_amount_and_note_and_reads_back() {
        let address = "olc409c2c2374917964bd7d010ddb77f90500eb3a41";
        assert_eq!(request_uri(address, None, " "), address, "nothing asked is the bare address");
        let uri = request_uri(address, Some(150_000_000), "Lunch & tea");
        assert_eq!(uri, format!("olc:{address}?amount=1.5&message=Lunch%20%26%20tea"));
        assert_eq!(parse_request(&uri), Some((address.to_string(), Some(150_000_000), "Lunch & tea".to_string())));
        assert_eq!(parse_request(address), None);
    }
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
