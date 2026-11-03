//! The computers this one is connected to, as Bitcoin Core's Peers tab lists them. Plain words in
//! basic mode; the whole table in expert mode. Either way a person can drop one, keep its address
//! out for an hour or a day, let it back, and connect to a new address.
use super::Coins;
use crate::core::host_peers::{Carried, PeerRow, PeersView};
use crate::core::{unix_ms, ChannelOrder, Command};
use crate::pages::Cx;
use crate::ui::explain;
use crate::ui::format;
use crate::ui::tokens;
use crate::ui::widgets::{self, button, button_on, card, cell, divider, field, header_cell, meta, mono, section, Columns, Kind};
use egui::{Align, Label, Layout, Ui};

/// Address, type, direction, ping, sent, received, version, since, and the buttons.
const WEIGHTS: [f32; 9] = [1.6, 0.6, 0.6, 0.7, 0.8, 0.8, 1.0, 0.9, 2.6];
const HOUR: u64 = 1;
const DAY: u64 = 24;

fn carried_text(cx: &Cx, carried: Carried) -> &'static str {
    cx.say(match carried {
        Carried::Tcp => "peers.by_tcp",
        Carried::Usb => "peers.by_usb",
        Carried::Radio => "peers.by_radio",
    })
}

/// Who it is: the name its channels go by, else where it connects from.
fn who(cx: &Cx, row: &PeerRow) -> String {
    match (row.name.is_empty(), row.key.is_empty(), row.address.is_empty()) {
        (false, _, _) => row.name.clone(),
        (true, false, _) => cx.fill("ch.unnamed", &[&row.key[..row.key.len().min(8)]]),
        (true, true, false) => cx.fill("peers.at", &[&row.address]),
        (true, true, true) => carried_text(cx, row.carried).to_string(),
    }
}

fn direction(cx: &Cx, row: &PeerRow) -> &'static str {
    cx.say(if row.inbound { "peers.in" } else { "peers.out" })
}

fn since(cx: &Cx, row: &PeerRow, now: u64) -> String {
    row.since_ms.map(|since| cx.fill("peers.since", &[&format::ago(cx.voice(), now, since)])).unwrap_or_default()
}

/// Disconnect and keep out, for a row on the network link.
fn buttons(ui: &mut Ui, cx: &Cx, row: &PeerRow) {
    let t = cx.t;
    let Some(number) = row.number else {
        return;
    };
    if button(ui, t, cx.say("peers.disconnect"), Kind::Quiet).clicked() {
        cx.send(Command::Channel(ChannelOrder::Disconnect(number)));
    }
    if row.address.is_empty() {
        return;
    }
    for (key, hours) in [("peers.ban_hour", HOUR), ("peers.ban_day", DAY)] {
        if button(ui, t, cx.say(key), Kind::Quiet).clicked() {
            cx.send(Command::Channel(ChannelOrder::Ban { address: row.address.clone(), hours }));
        }
    }
}

fn plain_list(ui: &mut Ui, cx: &Cx, view: &PeersView) {
    let t = cx.t;
    let now = unix_ms();
    for row in &view.rows {
        ui.horizontal(|ui| {
            ui.set_min_height(tokens::ROW);
            ui.vertical(|ui| {
                ui.label(widgets::medium_text(who(cx, row)));
                let mut how = vec![carried_text(cx, row.carried).to_string()];
                if row.number.is_some() {
                    how.push(direction(cx, row).to_string());
                }
                let since = since(cx, row, now);
                if !since.is_empty() {
                    how.push(since);
                }
                meta(ui, &how.join(" · "), t);
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // Right to left, so the order reads the other way round.
                ui.horizontal(|ui| buttons(ui, cx, row));
            });
        });
        divider(ui, t);
    }
}

fn expert_table(ui: &mut Ui, cx: &Cx, view: &PeersView) {
    let t = cx.t;
    let now = unix_ms();
    let columns = Columns::new(ui, &WEIGHTS);
    let heads = [
        "pro.peer_address", "pro.peer_type", "pro.peer_direction", "pro.peer_ping", "pro.peer_sent",
        "pro.peer_received", "pro.peer_version", "pro.peer_since",
    ];
    columns.row(ui, |ui, index| {
        if let Some(key) = heads.get(index) {
            header_cell(ui, t, cx.say(key));
        }
    });
    divider(ui, t);
    for row in &view.rows {
        columns.row(ui, |ui, index| match index {
            0 => {
                let shown = if row.address.is_empty() { who(cx, row) } else { row.address.clone() };
                let label = ui.add(Label::new(mono(shown)).truncate());
                if !row.key.is_empty() {
                    label.on_hover_text(format!("{} {}", row.name, row.key));
                }
            }
            1 => cell(ui, row.carried.word()),
            2 if row.number.is_some() => cell(ui, direction(cx, row)),
            3 => cell(ui, row.ping_us.map(|us| cx.fill("pro.ms", &[&format!("{:.1}", us as f64 / 1_000.0)])).unwrap_or_default()),
            4 if row.number.is_some() => cell(ui, format::mebibytes(row.sent)),
            5 if row.number.is_some() => cell(ui, format::mebibytes(row.received)),
            6 => {
                ui.add(Label::new(row.build.as_str()).truncate());
            }
            7 => cell(ui, row.since_ms.map(|since| format::ago(cx.voice(), now, since)).unwrap_or_default()),
            8 => buttons(ui, cx, row),
            _ => {}
        });
    }
}

impl Coins {
    /// Who this computer is connected to, and connecting to a new address.
    pub(super) fn peers(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        let view = &cx.snap.node.channels.network;
        card(ui, t, |ui| {
            section(ui, cx.say("peers.title"));
            explain::short(ui, t, cx.say("peers.about"));
            ui.add_space(tokens::GAP);
            match (view.rows.is_empty(), cx.expert()) {
                (true, _) => {
                    meta(ui, cx.say("peers.none"), t);
                }
                (false, false) => plain_list(ui, cx, view),
                (false, true) => expert_table(ui, cx, view),
            }
            if !view.banned.is_empty() {
                ui.add_space(tokens::GAP);
                meta(ui, cx.say("peers.banned_title"), t);
                for (address, left) in &view.banned {
                    ui.horizontal(|ui| {
                        ui.label(cx.fill("peers.banned_row", &[address, &format::span(cx.voice(), *left)]));
                        if button(ui, t, cx.say("peers.unban"), Kind::Quiet).clicked() {
                            cx.send(Command::Channel(ChannelOrder::Unban(address.clone())));
                        }
                    });
                }
            }
            ui.add_space(tokens::GAP);
            self.add_address(ui, cx);
        });
    }

    /// Connects to an address typed in, and keeps it for next time.
    fn add_address(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        if !cx.expert() && !self.by_address {
            if button(ui, t, cx.say("coins.by_address"), Kind::Plain).clicked() {
                self.by_address = true;
            }
            return;
        }
        ui.horizontal(|ui| {
            let hint = cx.say("coins.address_hint");
            let response = field(ui, &mut self.address, hint, 280.0);
            let enter = response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            let ready = !self.address.trim().is_empty();
            let add = button_on(ui, t, cx.say("coins.add"), Kind::Plain, ready).clicked();
            if ready && (enter || add) {
                cx.send(Command::AddAddress(std::mem::take(&mut self.address)));
            }
        });
    }
}
