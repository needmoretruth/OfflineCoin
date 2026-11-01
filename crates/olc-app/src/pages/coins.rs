//! This coin, the coins found around, and starting or joining another. Nothing joins until a
//! person picks it.
use super::coin_new::CoinForm;
use super::{Cx, Page, Slipping};
use crate::core::{Command, Personal};
use crate::discovery::{ChainGroup, CoinGroup, Entry, Identity, Reach, Relation, Work};
use crate::ui::floating::{sheet, sheet_head};
use crate::ui::picker::{Picked, Picker};
use crate::ui::tokens;
use crate::ui::widgets::{
    self, button, button_on, card, cell, divider, dot, field, header_cell, meta, mono, section, tag,
    Columns, Kind,
};
use crate::ui::{format, Icon};
use egui::{Align, Layout, Ui};
use olc_core::CoinConfig;
use olc_node::Medium;

const WEIGHTS: [f32; 7] = [1.5, 2.3, 0.9, 1.4, 0.8, 1.8, 0.8];
const COIN_ENDING: &str = "coin";

enum Filing {
    Join(Picker),
    Save(Picker),
}

#[derive(Default)]
pub struct Coins {
    address: String,
    by_address: bool,
    new_coin: Option<CoinForm>,
    filing: Option<Filing>,
}

/// Shown only when this node cannot go on by itself. Taking a peer's word is the person's call.
fn history_card(ui: &mut Ui, cx: &mut Cx) {
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
    let oldest = format::number(oldest.into());
    let mut slip = None;
    card(ui, t, |ui| {
        section(ui, cx.say("history.title"));
        ui.label(cx.fill("history.body", &[&oldest]));
        if button(ui, t, cx.say("history.read"), Kind::Main).clicked() {
            slip = Some(cx.slip_for(Slipping {
                what: cx.say("history.title"),
                rows: vec![(cx.say("history.from"), oldest.clone())],
                warning: cx.say("history.trust"),
                sign: cx.say("history.accept"),
                danger: true,
                order: vec![Command::Own(Personal::AcceptSnapshot)],
            }));
        }
    });
    if slip.is_some() {
        *cx.slip = slip;
    }
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
        true => cx.fill("pro.age_seconds", &[&seconds.to_string()]),
        false => cx.fill("pro.age_minutes", &[&(seconds / 60).to_string()]),
    }
}

fn work_cell(ui: &mut Ui, cx: &Cx, entry: &Entry) {
    let t = cx.t;
    let (colour, text) = match entry.work {
        Work::Mining(rate) => (t.good, cx.fill("work.mining", &[&format::rate(cx.voice(), rate)])),
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

/// One thing found, for an expert.
fn entry_row(ui: &mut Ui, cx: &Cx, columns: &Columns, entry: &Entry, group: (Option<&Identity>, Relation)) {
    let (identity, relation) = group;
    columns.row(ui, |ui, index| match index {
        0 => {
            let shown = ui.add(egui::Label::new(reach_text(cx, &entry.reach)).truncate());
            if let Some(address) = &entry.address {
                shown.on_hover_text(address);
            }
        }
        1 => {
            let device = format::device(cx.voice(), &entry.device);
            let text = match entry.name.is_empty() {
                true => device,
                false => format!("{device}  {}", entry.name),
            };
            cell(ui, text);
        }
        2 => cell(ui, dash(entry.height.map(|height| format::number(height.into())))),
        3 => cell(ui, dash(entry.work_done.map(format::number))),
        4 => cell(ui, dash(Some(entry.build.clone()).filter(|build| !build.is_empty()))),
        5 => work_cell(ui, cx, entry),
        _ => connect_button(ui, cx, entry, identity, relation),
    });
}

/// Connects to one more node of this same coin.
fn connect_button(ui: &mut Ui, cx: &Cx, entry: &Entry, identity: Option<&Identity>, relation: Relation) {
    let Some(identity) = identity else {
        return;
    };
    let own = matches!(entry.reach, Reach::ThisApp | Reach::ThisComputer);
    if own || relation != Relation::Yours || entry.address.is_none() {
        return;
    }
    if button(ui, cx.t, cx.say("pro.connect"), Kind::Plain).clicked() {
        cx.send(Command::Join { coin: Box::new(identity.coin.clone()), address: entry.address.clone() });
    }
}

fn chain_tags(ui: &mut Ui, cx: &Cx, chain: &ChainGroup, many: bool) {
    let t = cx.t;
    let (colour, key) = match chain.relation {
        Relation::Yours => (t.good, Some("coins.yours")),
        Relation::Split => (t.warn, Some("coins.split")),
        Relation::Network if many => (t.muted, Some("pro.same_coin_chain")),
        _ => (t.muted, None),
    };
    if let Some(key) = key {
        tag(ui, cx.say(key), colour, t);
    }
    if chain.tips > 1 {
        let text = cx.fill("pro.tips", &[&chain.tips.to_string()]);
        tag(ui, &text, t.warn, t);
    }
}

fn expert_table(ui: &mut Ui, cx: &Cx, group: &CoinGroup) {
    let t = cx.t;
    let many = group.chains.len() > 1;
    let columns = Columns::new(ui, &WEIGHTS);
    for chain in &group.chains {
        ui.horizontal(|ui| chain_tags(ui, cx, chain, many));
        columns.row(ui, |ui, index| {
            let keys = ["pro.reach", "pro.device", "pro.height", "pro.work", "pro.build", "pro.activity"];
            if let Some(key) = keys.get(index) {
                header_cell(ui, t, cx.say(key));
            }
        });
        divider(ui, t);
        for entry in &chain.entries {
            entry_row(ui, cx, &columns, entry, (group.identity.as_ref(), chain.relation));
        }
    }
    if let Some(identity) = &group.identity {
        let id = mono(format::short_id(&identity.genesis)).color(t.muted);
        ui.label(id).on_hover_text(cx.say("pro.genesis"));
    }
}

/// A coin that is not this one, with where to reach it.
struct Found<'a> {
    coin: &'a CoinConfig,
    count: usize,
    address: Option<String>,
    split: bool,
}

fn found_in(group: &CoinGroup) -> Option<Found<'_>> {
    let identity = group.identity.as_ref()?;
    let others = || group.chains.iter().filter(|chain| chain.relation != Relation::Yours);
    let around: Vec<&Entry> = others().flat_map(|chain| &chain.entries).collect();
    let first = around.first()?;
    Some(Found {
        coin: &identity.coin,
        count: around.len(),
        address: first.address.clone(),
        split: others().any(|chain| chain.relation == Relation::Split),
    })
}

impl Coins {
    fn this_coin(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        let node = &cx.snap.node;
        card(ui, t, |ui| {
            ui.horizontal(|ui| {
                dot(ui, t.good);
                if let Some(coin) = &node.coin {
                    section(ui, &coin.name);
                    tag(ui, &coin.ticker, t.muted, t);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if button(ui, t, cx.say("coins.save_file"), Kind::Quiet).clicked() {
                        let picker = Picker::save("coins.save_file", "coin", COIN_ENDING, cx.folder.as_deref());
                        self.filing = Some(Filing::Save(picker));
                    }
                });
            });
            meta(ui, &cx.fill("coins.this_line", &[&format::number(node.height.into()), &node.peers.to_string()]), t);
        });
    }

    fn found(&self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        let snap = cx.snap;
        let found: Vec<Found> = snap.groups.iter().filter_map(found_in).collect();
        section(ui, cx.say("coins.found"));
        if found.is_empty() {
            let again = cx.say("coins.look_again");
            if widgets::empty(ui, t, cx.say("coins.none"), cx.say("coins.none_body"), Some(again)) {
                cx.send(Command::Rescan);
            }
            return;
        }
        let mut slip = None;
        for item in &found {
            let name = &item.coin.name;
            let line = cx.fill("coins.found_about", &[&item.coin.ticker, &item.count.to_string()]);
            ui.horizontal(|ui| {
                ui.set_min_height(tokens::ROW);
                ui.label(widgets::medium_text(name.clone()));
                meta(ui, &line, t);
                if item.split {
                    tag(ui, cx.say("coins.split"), t.warn, t);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if button(ui, t, cx.say("coins.join"), Kind::Plain).clicked() {
                        slip = Some(cx.slip_for(Slipping {
                            what: cx.say("coins.join"),
                            rows: vec![(cx.say("coin.name"), name.clone())],
                            warning: cx.say("coins.join_warning"),
                            sign: cx.say("coins.join"),
                            danger: false,
                            order: vec![Command::Join {
                                coin: Box::new(item.coin.clone()),
                                address: item.address.clone(),
                            }],
                        }));
                    }
                });
            });
            widgets::divider(ui, t);
        }
        if slip.is_some() {
            *cx.slip = slip;
        }
    }

    fn start_buttons(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        ui.horizontal(|ui| {
            if button(ui, t, cx.say("coins.new"), Kind::Plain).clicked() {
                self.new_coin = Some(CoinForm::default());
            }
            if button(ui, t, cx.say("coins.join_file"), Kind::Plain).clicked() {
                let picker = Picker::open("coins.join_file", COIN_ENDING, cx.folder.as_deref());
                self.filing = Some(Filing::Join(picker));
            }
            if button(ui, t, cx.say("coins.look_again"), Kind::Quiet).clicked() {
                cx.send(Command::Rescan);
            }
        });
        if !cx.expert() && !self.by_address {
            if button(ui, t, cx.say("coins.by_address"), Kind::Quiet).clicked() {
                self.by_address = true;
            }
            return;
        }
        ui.add_space(tokens::GAP);
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

    fn new_coin_sheet(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        let Some(form) = &mut self.new_coin else {
            return;
        };
        let (done, outside) = sheet(ui, t, "new-coin", tokens::SHEET_WIDE, |ui| {
            sheet_head(ui, cx.say("coins.new"));
            form.show(ui, cx);
            ui.add_space(tokens::GAP_WIDE);
            let coin = form.coin(cx);
            let mut done = false;
            ui.horizontal(|ui| {
                let start = button_on(ui, t, cx.say("coins.start"), Kind::Main, coin.is_some());
                if let Some(coin) = coin.filter(|_| start.clicked()) {
                    cx.send(Command::Join { coin: Box::new(coin), address: None });
                    done = true;
                }
                done |= button(ui, t, cx.say("common.cancel"), Kind::Plain).clicked();
            });
            done
        });
        if done || outside {
            self.new_coin = None;
        }
    }

    fn file_sheet(&mut self, ui: &Ui, cx: &mut Cx) {
        let voice = cx.voice();
        let t = cx.t;
        let answer = match &mut self.filing {
            None => return,
            Some(Filing::Join(picker) | Filing::Save(picker)) => picker.show(ui, t, voice),
        };
        match (answer, &self.filing) {
            (Picked::Waiting, _) => return,
            (Picked::Chosen(path), Some(filing)) => {
                *cx.folder = path.parent().map(std::path::Path::to_path_buf);
                let path = path.display().to_string();
                cx.send(match filing {
                    Filing::Join(_) => Command::JoinCoinFile(path),
                    Filing::Save(_) => Command::SaveCoinFile(path),
                });
            }
            _ => {}
        }
        self.filing = None;
    }
}

impl Page for Coins {
    fn title(&self) -> &'static str {
        "page.coins"
    }

    fn icon(&self) -> Icon {
        Icon::Network
    }

    fn show(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        widgets::title(ui, cx.say("page.coins"));
        widgets::slot(ui, |ui| history_card(ui, cx));
        self.this_coin(ui, cx);
        ui.add_space(tokens::GAP_WIDE);
        widgets::slot(ui, |ui| self.found(ui, cx));
        ui.add_space(tokens::GAP_WIDE);
        self.start_buttons(ui, cx);
        if cx.expert() {
            ui.add_space(tokens::GAP_WIDE);
            for group in &cx.snap.groups {
                card(ui, t, |ui| {
                    match &group.identity {
                        Some(identity) => section(ui, &identity.coin.name),
                        None => section(ui, cx.say("pro.unknown_coin")),
                    }
                    expert_table(ui, cx, group);
                });
                ui.add_space(tokens::GAP);
            }
            meta(ui, &scan_line(cx), t);
        }
        self.new_coin_sheet(ui, cx);
        self.file_sheet(ui, cx);
    }
}

fn scan_line(cx: &Cx) -> String {
    let scan = &cx.snap.scan;
    match (scan.running, scan.age_secs) {
        (true, _) => cx.say("pro.looking").to_string(),
        (false, Some(age)) if scan.subnets.is_empty() => cx.fill("pro.looked_local", &[&age.to_string()]),
        (false, Some(age)) => cx.fill(
            "pro.looked",
            &[&age.to_string(), &format::number(scan.tried as u128), &scan.subnets.len().to_string()],
        ),
        (false, None) => cx.say("pro.not_looked").to_string(),
    }
}
