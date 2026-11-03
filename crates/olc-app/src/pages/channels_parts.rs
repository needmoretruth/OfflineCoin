//! The parts of the channels page that let two people find each other: this computer's channel
//! address, the one field that takes the other's, what each channel can send and receive, and the
//! requests made here.
use super::{hex, name_of, Channels, WIDE};
use crate::core::host_channels::{read_channel_address, ChannelView, ChannelsView, InvoiceView};
use crate::core::{parse_coins, unix_ms, ChannelOrder, Command};
use crate::pages::money::code;
use crate::pages::{Cx, Slipping};
use crate::ui::explain;
use crate::ui::format;
use crate::ui::tokens;
use crate::ui::widgets::{self, button, button_on, card, field, meta, mono, pick_row, section, tag, Kind};
use egui::{vec2, Align, CornerRadius, Label, Layout, Rect, Sense, TextEdit, Ui};
use olc_service::qr::{qr_grid, QrGrid};

/// How many requests the list shows before the rest fold away.
const REQUESTS: usize = 6;
const BAR: f32 = 18.0;
const BAR_SECONDS: f32 = 0.5;

/// What the open field holds, read.
enum Typed {
    Empty,
    /// A node heard lately, by its key or the start of it.
    Nearby(String),
    /// A whole key, with where to dial it or not.
    Address,
    /// `host:port` alone: something to connect to, not yet to open a channel with.
    Place(String),
    Wrong(&'static str),
}

fn typed(text: &str, view: &ChannelsView) -> Typed {
    let text = text.trim();
    if text.is_empty() {
        return Typed::Empty;
    }
    if let Some((key, _)) = view.peers.iter().find(|(key, name)| key == text || (!name.is_empty() && name == text)) {
        return Typed::Nearby(key.clone());
    }
    match read_channel_address(text) {
        Ok(_) => Typed::Address,
        Err(_) if !text.contains('@') && text.contains(':') && !text.chars().all(|c| c.is_ascii_hexdigit()) => {
            Typed::Place(text.to_string())
        }
        Err(key) => Typed::Wrong(key),
    }
}

/// A field with its name above it, so the name stays when the field holds a value.
/// The faint text inside shows what an empty field means, such as the usual wait.
fn labelled(ui: &mut Ui, cx: &Cx, key: &str, text: &mut String, empty_means: &str, width: f32) {
    let name = cx.say(key);
    ui.vertical(|ui| {
        ui.set_width(width);
        meta(ui, name, cx.t);
        let edit = TextEdit::singleline(text)
            .hint_text(empty_means)
            .desired_width(width)
            .margin(vec2(10.0, 8.0))
            .min_size(vec2(width, tokens::CONTROL));
        ui.add(edit).accessible_name(name);
    });
}

/// A button level with fields that carry their names above them.
fn under_names(ui: &mut Ui, t: &tokens::Tokens, named: bool, add: impl FnOnce(&mut Ui)) {
    match named {
        true => {
            ui.vertical(|ui| {
                meta(ui, " ", t);
                add(ui);
            });
        }
        false => add(ui),
    }
}

/// `key@host:port` on one line: the start and end of the key, and the whole place to dial.
fn short_address(address: &str) -> String {
    match address.split_once('@') {
        Some((key, at)) if key.len() > 10 => format!("{}...{}@{at}", &key[..6], &key[key.len() - 4..]),
        _ => address.to_string(),
    }
}

/// A long key with its middle left out, as a label.
fn shortened(text: &str) -> String {
    let count = text.chars().count();
    if count <= 28 {
        return text.to_string();
    }
    let head: String = text.chars().take(12).collect();
    let tail: String = text.chars().skip(count - 12).collect();
    format!("{head}...{tail}")
}

impl Channels {
    /// What another person pastes to open a channel with this computer, with a copy button and a
    /// code a phone can read.
    pub(super) fn my_address(&mut self, ui: &mut Ui, cx: &mut Cx, view: &ChannelsView) {
        let t = cx.t;
        card(ui, t, |ui| {
            ui.set_max_width(WIDE);
            section(ui, cx.say("ch.my_address"));
            if view.address.is_empty() {
                explain::short(ui, t, cx.say("ch.no_address"));
                if cx.expert() {
                    meta(ui, &cx.fill("ch.my_key", &[&view.key]), t);
                }
                return;
            }
            explain::short(ui, t, cx.say("ch.my_address_about"));
            ui.add_space(tokens::GAP);
            if self.address_code.as_ref().is_none_or(|(text, _)| *text != view.address) {
                self.address_code = Some((view.address.clone(), qr_grid(&view.address)));
            }
            // One line: in basic mode the key's middle is left out, the place to dial is whole.
            // Copy and the code always carry all of it.
            let shown = if cx.expert() { view.address.clone() } else { short_address(&view.address) };
            ui.add(Label::new(mono(shown)).truncate());
            ui.add_space(tokens::GAP);
            ui.horizontal(|ui| {
                if button(ui, t, cx.say("ch.copy"), Kind::Plain).clicked() {
                    ui.ctx().copy_text(view.address.clone());
                    self.copied_at = Some(ui.input(|input| input.time));
                }
                let label = if self.show_address_code { "ch.hide_code" } else { "ch.show_code" };
                if button(ui, t, cx.say(label), Kind::Quiet).clicked() {
                    self.show_address_code = !self.show_address_code;
                }
                let now = ui.input(|input| input.time);
                if self.copied_at.is_some_and(|at| now - at < 3.0) {
                    meta(ui, cx.say("ch.copied"), t);
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
                }
            });
            if let Some((_, Some(grid))) = self.address_code.as_ref().filter(|_| self.show_address_code) {
                ui.add_space(tokens::GAP);
                code(ui, grid, cx.say("ch.address_code"));
            }
            if view.network.only_here {
                ui.add_space(tokens::GAP);
                explain::short(ui, t, cx.say("ch.only_here"));
                if button(ui, t, cx.say("ch.listen_on"), Kind::Plain).clicked() {
                    cx.settings.listen_lan = true;
                    cx.save();
                }
            }
        });
    }

    /// One field for the other side's channel address. It connects and opens in one step.
    pub(super) fn open(&mut self, ui: &mut Ui, cx: &mut Cx, view: &ChannelsView) {
        let t = cx.t;
        section(ui, cx.say("ch.open"));
        explain::short(ui, t, cx.say("ch.open_about"));
        ui.add_space(tokens::GAP);
        let response = field(ui, &mut self.open_with, cx.say("ch.their_address"), WIDE);
        if response.changed() {
            self.open_fault = None;
        }
        let read = typed(&self.open_with, view);
        let amount = parse_coins(self.open_amount.trim()).filter(|units| *units > 0);
        let push = match self.open_push.trim() {
            "" => Some(0),
            pushed => parse_coins(pushed).filter(|pushed| amount.is_none_or(|amount| *pushed < amount)),
        };
        ui.add_space(4.0);
        let expert = cx.expert();
        let ready = amount.is_some() && push.is_some() && matches!(read, Typed::Nearby(_) | Typed::Address);
        let (mut go, mut connect) = (false, false);
        ui.horizontal(|ui| {
            // Expert mode has three numbers side by side, so each keeps its name above it, and the
            // buttons get an empty line above them to sit level with the fields.
            if expert {
                labelled(ui, cx, "ch.open_amount", &mut self.open_amount, "", 150.0);
                labelled(ui, cx, "ch.push", &mut self.open_push, "0", 150.0);
                labelled(ui, cx, "ch.delay", &mut self.open_delay, "144", 170.0);
            } else {
                field(ui, &mut self.open_amount, cx.say("ch.open_amount"), 150.0);
            }
            under_names(ui, t, expert, |ui| {
                go = button_on(ui, t, cx.say("ch.open_go"), Kind::Main, ready).clicked() && ready;
            });
            if matches!(read, Typed::Place(_)) {
                under_names(ui, t, expert, |ui| connect = button(ui, t, cx.say("ch.connect_only"), Kind::Plain).clicked());
            }
        });
        if go {
            self.open_slip(cx, view, &read);
        }
        if let (true, Typed::Place(place)) = (connect, &read) {
            cx.send(Command::AddAddress(place.clone()));
            self.open_with.clear();
        }
        let said = match &read {
            Typed::Wrong(key) => Some(cx.fill(key, &[&shortened(self.open_with.trim())])),
            Typed::Place(_) => Some(cx.say("ch.place_only").to_string()),
            _ if push.is_none() => Some(cx.say("ch.bad_push").to_string()),
            _ => self.open_fault.clone(),
        };
        if let Some(said) = said {
            meta(ui, &said, t);
        }
        for address in &view.reaching {
            meta(ui, &cx.fill("ch.reaching", &[address]), t);
        }
        if cx.expert() {
            explain::short(ui, t, cx.say("ch.push_about"));
        }
        ui.add_space(tokens::GAP_WIDE);
        self.nearby(ui, cx, view);
    }

    fn open_slip(&mut self, cx: &mut Cx, view: &ChannelsView, read: &Typed) {
        let typed = self.open_with.trim().to_string();
        let shown = match read {
            Typed::Nearby(key) => {
                let name = view.peers.iter().find(|(known, _)| known == key).map(|(_, name)| name.clone());
                name_of(cx, &name.unwrap_or_default(), key)
            }
            _ => shortened(&typed),
        };
        let mut rows = vec![(cx.say("ch.other"), shown), (cx.say("ch.amount"), self.open_amount.trim().to_string())];
        if !self.open_push.trim().is_empty() {
            rows.push((cx.say("ch.push"), self.open_push.trim().to_string()));
        }
        let peer = match read {
            Typed::Nearby(key) => key.clone(),
            _ => typed,
        };
        *cx.slip = Some(cx.slip_for(Slipping {
            what: cx.say("ch.open"),
            rows,
            warning: cx.say("ch.open_warning"),
            sign: cx.say("ch.open_go"),
            danger: false,
            order: vec![Command::Channel(ChannelOrder::Open {
                peer,
                amount: self.open_amount.trim().to_string(),
                delay: self.open_delay.trim().to_string(),
                push: self.open_push.trim().to_string(),
            })],
        }));
        self.open_with.clear();
        self.open_amount.clear();
        self.open_push.clear();
    }

    /// Computers already connected that hold channels: a press puts one in the field. With none,
    /// nothing is said: the field above is all a person needs.
    fn nearby(&mut self, ui: &mut Ui, cx: &Cx, view: &ChannelsView) {
        let t = cx.t;
        if view.peers.is_empty() {
            return;
        }
        meta(ui, cx.say("ch.pick"), t);
        ui.add_space(4.0);
        for (key, name) in &view.peers {
            let shown = name_of(cx, name, key);
            let current = self.open_with.trim() == key;
            // A label that can be selected would take the press meant for the row.
            let row = pick_row(ui, t, tokens::ROW, current, &shown, |ui| {
                ui.add(Label::new(shown.clone()).truncate().selectable(false));
            });
            if row.clicked() {
                self.open_with = key.clone();
            }
        }
    }

    /// What an open channel can send and receive, and behind Details the two shares as one bar.
    pub(super) fn room(ui: &mut Ui, cx: &Cx, channel: &ChannelView) {
        let t = cx.t;
        let ticker = cx.ticker();
        ui.horizontal(|ui| {
            ui.label(cx.fill("ch.can_send", &[&format::coins(channel.can_send, ticker)]));
            ui.add_space(tokens::GAP_WIDE);
            ui.label(cx.fill("ch.can_receive", &[&format::coins(channel.can_receive, ticker)]));
        });
        let labels = (cx.say("explain.details"), cx.say("explain.hide"));
        explain::details(ui, t, ("room", hex(&channel.id)), labels, |ui| {
            share_bar(ui, cx, channel);
            ui.add_space(tokens::GAP);
            ui.label(cx.say("ch.room_about"));
            ui.add_space(4.0);
            explain::analogy(ui, t, cx.say("ch.room_analogy"));
        });
    }

    /// The requests made here, newest first, each with how it stands.
    pub(super) fn requests(&mut self, ui: &mut Ui, cx: &Cx, view: &ChannelsView) {
        let t = cx.t;
        if view.invoices.is_empty() {
            return;
        }
        ui.add_space(tokens::GAP_WIDE);
        section(ui, cx.say("ch.requests"));
        ui.add_space(4.0);
        let now = unix_ms();
        for invoice in view.invoices.iter().take(REQUESTS) {
            request_row(ui, cx, invoice, now);
        }
        if view.invoices.len() > REQUESTS {
            meta(ui, &cx.fill("ch.requests_more", &[&(view.invoices.len() - REQUESTS).to_string()]), t);
        }
    }

    /// The code and text of the newest request still waiting, to pass on to the payer.
    pub(super) fn waiting_code(&mut self, ui: &mut Ui, cx: &Cx, view: &ChannelsView) {
        let t = cx.t;
        let Some(newest) = view.invoices.first().filter(|newest| !newest.paid && !newest.expired) else {
            return;
        };
        if self.code.as_ref().is_none_or(|(text, _): &(String, Option<QrGrid>)| *text != newest.text) {
            self.code = Some((newest.text.clone(), qr_grid(&newest.text)));
        }
        ui.add_space(tokens::GAP);
        card(ui, t, |ui| {
            ui.set_max_width(WIDE);
            ui.label(widgets::strong_text(format::coins(newest.amount, cx.ticker())));
            meta(ui, cx.say("ch.request_waiting"), t);
            ui.add_space(tokens::GAP);
            if let Some((_, Some(grid))) = &self.code {
                code(ui, grid, cx.say("ch.code"));
            }
            ui.add_space(tokens::GAP);
            ui.add(Label::new(mono(newest.text.as_str())).truncate());
            ui.add_space(4.0);
            if button(ui, t, cx.say("ch.copy"), Kind::Plain).clicked() {
                ui.ctx().copy_text(newest.text.clone());
            }
        });
    }
}

fn request_row(ui: &mut Ui, cx: &Cx, invoice: &InvoiceView, now_ms: u64) {
    let t = cx.t;
    let (key, colour) = match (invoice.paid, invoice.expired) {
        (true, _) => ("ch.status_paid", t.good),
        (false, true) => ("ch.status_expired", t.muted),
        (false, false) => ("ch.status_waiting", t.warn),
    };
    ui.horizontal(|ui| {
        ui.set_min_height(tokens::ROW);
        ui.label(widgets::medium_text(format::coins(invoice.amount, cx.ticker())));
        if !invoice.note.is_empty() {
            ui.add(Label::new(invoice.note.as_str()).truncate());
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            tag(ui, cx.say(key), colour, t);
            meta(ui, &format::ago(cx.voice(), now_ms, invoice.made_s.saturating_mul(1_000)), t);
        });
    });
    widgets::divider(ui, t);
}

/// Both shares of a channel as one bar: mine from the left, theirs from the right. A payment
/// slides the line between them.
fn share_bar(ui: &mut Ui, cx: &Cx, channel: &ChannelView) {
    let t = cx.t;
    let total = channel.ours.saturating_add(channel.theirs).max(1);
    let wanted = channel.ours as f32 / total as f32;
    let id = ui.make_persistent_id(("share", hex(&channel.id)));
    let shown = ui.ctx().animate_value_with_time(id, wanted, BAR_SECONDS);
    let width = ui.available_width().min(WIDE - 2.0 * tokens::GAP_WIDE);
    let (rect, response) = ui.allocate_exact_size(vec2(width, BAR), Sense::hover());
    let painter = ui.painter();
    let radius = CornerRadius::same(tokens::RADIUS_TAG);
    painter.rect_filled(rect, radius, t.line_strong);
    let mine = Rect::from_min_size(rect.min, vec2(rect.width() * shown, rect.height()));
    painter.rect_filled(mine, radius, t.main_top);
    let label = cx.fill("ch.bar", &[&format::coins(channel.ours, cx.ticker()), &format::coins(channel.theirs, cx.ticker())]);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, label.as_str()));
    // The two shares sit under the two ends of the bar, however wide the page is.
    ui.allocate_ui(vec2(width, 0.0), |ui| ui.horizontal(|ui| {
        ui.set_width(width);
        ui.label(cx.fill("ch.yours", &[&format::coins(channel.ours, cx.ticker())]));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(cx.fill("ch.theirs", &[&format::coins(channel.theirs, cx.ticker())]));
        });
    }));
}
