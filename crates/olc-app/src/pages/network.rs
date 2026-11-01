//! Everything found, sorted into coins and chains. Nothing joins until a person picks it.
use super::{Cx, Page};
use crate::core::{Command, Personal};
use crate::discovery::{ChainGroup, CoinGroup, Entry, Identity, Reach, Relation, Work};
use crate::ui::format;
use crate::ui::tokens;
use crate::ui::widgets::{
    self, button, card, cell, chip, divider, dot, field, header_cell, meta, mono, section,
    Columns, Kind,
};
use crate::ui::Icon;
use egui::{Align, Layout, Ui};
use olc_node::Medium;

const WEIGHTS: [f32; 7] = [1.5, 2.3, 0.9, 1.4, 0.8, 1.8, 0.8];

#[derive(Default)]
pub struct Network {
    address: String,
}

/// Shown only when this node cannot go on by itself. Taking a peer's word is the person's call.
fn history_card(ui: &mut Ui, cx: &Cx) {
    let t = cx.t;
    let node = &cx.snap.node;
    if node.assumed_from.is_some() {
        meta(ui, cx.say("history.assumed"), t);
    }
    if let Some((got, parts)) = node.fetching {
        meta(ui, &cx.fill("history.fetching", &[&got.to_string(), &parts.to_string()]), t);
    }
    let Some((_, oldest)) = node.needs_history.filter(|_| node.fetching.is_none()) else {
        return;
    };
    card(ui, t, |ui| {
        section(ui, cx.say("history.title"));
        ui.label(cx.fill("history.body", &[&format::number(oldest.into())]));
        ui.label(cx.say("history.trust"));
        if button(ui, t, cx.say("history.accept"), Kind::Main).clicked() {
            cx.send(Command::Own(Personal::AcceptSnapshot));
        }
    });
    ui.add_space(tokens::GAP);
}

pub fn medium_text(cx: &Cx, medium: Medium) -> &'static str {
    cx.say(match medium {
        Medium::Wire => "reach.wired",
        Medium::EspNow => "reach.espnow",
        Medium::Bluetooth => "reach.bluetooth",
        Medium::Wifi => "reach.wifi",
    })
}

pub fn reach_text(cx: &Cx, reach: &Reach) -> String {
    match reach {
        Reach::ThisApp => cx.say("reach.this_app").into(),
        Reach::ThisComputer => cx.say("reach.this_computer").into(),
        Reach::Usb => cx.say("reach.usb").into(),
        Reach::Radio(Some(board)) => cx.fill("reach.radio_through", &[board]),
        Reach::Radio(None) => cx.say("reach.radio").into(),
        Reach::Link(medium, Some(board)) => cx.fill("reach.link_through", &[board, medium_text(cx, *medium)]),
        Reach::Link(medium, None) => medium_text(cx, *medium).into(),
        Reach::Lan => cx.say("reach.lan").into(),
        Reach::Internet => cx.say("reach.internet").into(),
    }
}

fn age_text(cx: &Cx, seconds: u64) -> String {
    match seconds < 60 {
        true => cx.fill("network.age_seconds", &[&seconds.to_string()]),
        false => cx.fill("network.age_minutes", &[&(seconds / 60).to_string()]),
    }
}

fn work_cell(ui: &mut Ui, cx: &Cx, entry: &Entry) {
    let t = cx.t;
    let (colour, text) = match entry.work {
        Work::Mining(rate) => (t.good, cx.fill("work.mining", &[&format::rate(rate)])),
        Work::Idle => (t.muted, cx.say("work.idle").to_string()),
        Work::Offline => (t.warn, cx.say("work.offline").to_string()),
        Work::Silent => (t.bad, cx.say("work.silent").to_string()),
        Work::Restarting => (t.muted, cx.say("work.restarting").to_string()),
    };
    dot(ui, colour);
    cell(ui, text);
    if let Some(age) = entry.age_secs {
        meta(ui, &age_text(cx, age), t);
    }
}

fn dash(value: Option<String>) -> String {
    value.unwrap_or_else(|| "-".into())
}

fn row(ui: &mut Ui, cx: &Cx, columns: &Columns, entry: &Entry, identity: Option<&Identity>, relation: Relation) {
    columns.row(ui, |ui, index| match index {
        0 => {
            let shown = ui.add(egui::Label::new(reach_text(cx, &entry.reach)).truncate());
            if let Some(address) = &entry.address {
                shown.on_hover_text(address);
            }
        }
        1 => {
            let device = format::device(cx.voice(), &entry.device);
            let text = if entry.name.is_empty() {
                device
            } else {
                format!("{device}  {}", entry.name)
            };
            cell(ui, text);
        }
        2 => cell(ui, dash(entry.height.map(|height| format::number(height.into())))),
        3 => cell(ui, dash(entry.work_done.map(format::number))),
        4 => cell(ui, dash(Some(entry.build.clone()).filter(|build| !build.is_empty()))),
        5 => work_cell(ui, cx, entry),
        _ => action(ui, cx, entry, identity, relation),
    });
}

fn action(ui: &mut Ui, cx: &Cx, entry: &Entry, identity: Option<&Identity>, relation: Relation) {
    let Some(identity) = identity else {
        return;
    };
    // This app's own nodes connect by themselves.
    let own = matches!(entry.reach, Reach::ThisApp | Reach::ThisComputer);
    if own && relation == Relation::Yours {
        return;
    }
    let key = if relation == Relation::Yours { "network.connect" } else { "network.join" };
    let usable = relation != Relation::Yours || entry.address.is_some();
    if usable && button(ui, cx.t, cx.say(key), Kind::Plain).clicked() {
        cx.send(Command::Join {
            coin: Box::new(identity.coin.clone()),
            address: entry.address.clone(),
        });
    }
}

fn chain_chips(ui: &mut Ui, cx: &Cx, chain: &ChainGroup, many: bool) {
    let t = cx.t;
    let (colour, key) = match chain.relation {
        Relation::Yours => (t.good, Some("network.yours")),
        Relation::Split => (t.warn, Some("network.split")),
        Relation::Network if many => (t.muted, Some("network.same_coin_chain")),
        _ => (t.muted, None),
    };
    if let Some(key) = key {
        chip(ui, cx.say(key), colour, t);
    }
    if chain.tips > 1 {
        let text = cx.fill("network.tips", &[&chain.tips.to_string()]);
        chip(ui, &text, t.warn, t);
    }
}

fn coin_heading(ui: &mut Ui, cx: &Cx, group: &CoinGroup, single: Option<&ChainGroup>) {
    let t = cx.t;
    ui.horizontal(|ui| {
        match &group.identity {
        Some(identity) => {
            section(ui, &identity.coin.name);
            chip(ui, &identity.coin.ticker, t.muted, t);
            ui.add_space(4.0);
            ui.label(mono(format::short_id(&identity.genesis)).color(t.muted))
                .on_hover_text(cx.say("network.genesis"));
        }
        None => section(ui, cx.say("network.unknown_coin")),
        }
        if let Some(chain) = single {
            ui.add_space(4.0);
            chain_chips(ui, cx, chain, false);
        }
    });
}

fn show_group(ui: &mut Ui, cx: &Cx, group: &CoinGroup) {
    let t = cx.t;
    card(ui, t, |ui| {
        let many = group.chains.len() > 1;
        let single = group.chains.first().filter(|_| !many);
        coin_heading(ui, cx, group, single);
        let columns = Columns::new(ui, &WEIGHTS);
        for chain in &group.chains {
            if many {
                ui.horizontal(|ui| chain_chips(ui, cx, chain, many));
            }
            columns.row(ui, |ui, index| {
                let keys = [
                    "column.reach",
                    "column.device",
                    "column.height",
                    "column.work",
                    "column.build",
                    "column.activity",
                ];
                if let Some(key) = keys.get(index) {
                    header_cell(ui, t, cx.say(key));
                }
            });
            divider(ui, t);
            for entry in &chain.entries {
                row(ui, cx, &columns, entry, group.identity.as_ref(), chain.relation);
            }
        }
    });
}

fn scan_line(cx: &Cx) -> String {
    let scan = &cx.snap.scan;
    let subnets = scan.subnets.join(", ");
    match (scan.running, scan.age_secs) {
        (true, _) => cx.say("network.looking").to_string(),
        (false, Some(age)) if subnets.is_empty() => {
            cx.fill("network.looked_local", &[&age.to_string()])
        }
        (false, Some(age)) => cx.fill(
            "network.looked",
            &[
                &age.to_string(),
                &format::number(scan.tried as u128),
                &scan.subnets.len().to_string(),
            ],
        ),
        (false, None) => cx.say("network.not_looked").to_string(),
    }
}

impl Page for Network {
    fn title(&self) -> &'static str {
        "page.network"
    }

    fn icon(&self) -> Icon {
        Icon::Network
    }

    fn show(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        widgets::title(ui, cx.say("page.network"));
        history_card(ui, cx);
        ui.horizontal(|ui| {
            let hint = cx.say("network.address_hint");
            let response = field(ui, &mut self.address, hint, 260.0);
            let enter = response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            let add = button(ui, t, cx.say("network.add"), Kind::Plain).clicked();
            if (enter || add) && !self.address.trim().is_empty() {
                cx.send(Command::AddAddress(std::mem::take(&mut self.address)));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if button(ui, t, cx.say("network.look_again"), Kind::Plain).clicked() {
                    cx.send(Command::Rescan);
                }
                ui.add_space(tokens::GAP);
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    let line = meta(ui, &scan_line(cx), t);
                    if !cx.snap.scan.subnets.is_empty() {
                        line.on_hover_text(cx.snap.scan.subnets.join("\n"));
                    }
                });
            });
        });
        ui.add_space(tokens::GAP);
        for group in &cx.snap.groups {
            show_group(ui, cx, group);
            ui.add_space(tokens::GAP_WIDE - tokens::GAP);
        }
        if cx.snap.groups.iter().map(CoinGroup::count).sum::<usize>() <= 1 {
            meta(ui, cx.say("network.nothing_else"), t);
        }
    }
}
