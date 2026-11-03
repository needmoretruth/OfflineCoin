//! This coin, the coins found around, and starting or joining another. Nothing joins until a
//! person picks it.
use super::coin_fork::ForkForm;
use super::coin_new::CoinForm;
use super::{Cx, Page, Slipping};
use crate::core::{Command, Personal};
use crate::discovery::{ChainGroup, CoinGroup, Entry, Identity, Reach, Relation, Work};
use crate::ui::floating::{sheet, sheet_head};
use crate::ui::picker::{Picked, Picker};
use crate::ui::tokens;
use crate::ui::widgets::{
    self, button, button_on, card, cell, divider, dot, header_cell, meta, mono, section, tag,
    Columns, Kind,
};
use crate::ui::{format, Icon};
use egui::{Align, Layout, ScrollArea, Ui};
use olc_core::CoinConfig;
use olc_node::{Keeps, Medium};

#[path = "coins_peers.rs"]
mod peers;

const WEIGHTS: [f32; 8] = [1.4, 2.0, 1.0, 0.8, 1.2, 0.7, 1.7, 0.8];
/// Device, what it keeps, what it does.
const PLAIN_WEIGHTS: [f32; 3] = [2.2, 1.0, 1.4];
const COIN_ENDING: &str = "coin";
/// A sheet's margins, title and buttons.
const SHEET_FRAME: f32 = 220.0;

enum Filing {
    Join(Picker),
    Save(Picker),
}

#[derive(Default)]
pub struct Coins {
    address: String,
    by_address: bool,
    new_coin: Option<CoinForm>,
    fork: Option<ForkForm>,
    filing: Option<Filing>,
}

/// Shown only when this node cannot go on by itself. Taking a peer's word is the person's call.
pub(super) fn history_card(ui: &mut Ui, cx: &mut Cx) {
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
        ui.label(cx.say("history.todo"));
        ui.add_space(tokens::GAP);
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
        Work::Mining(0) => (t.good, cx.say("state.mining").to_string()),
        Work::Mining(rate) => (t.good, cx.fill("work.mining", &[&format::rate(cx.voice(), rate)])),
        Work::Cooling(rate) => (t.warn, cx.fill("work.cooling", &[&format::rate(cx.voice(), rate)])),
        Work::Idle => (t.muted, cx.say("work.idle").to_string()),
        Work::Offline => (t.warn, cx.say("work.offline").to_string()),
        Work::Silent => (t.bad, cx.say("work.silent").to_string()),
        Work::Restarting => (t.muted, cx.say("work.restarting").to_string()),
    };
    dot(ui, colour);
    match entry.age_secs {
        Some(age) => cell(ui, format!("{text}, {}", age_text(cx, age))),
        None => cell(ui, text),
    }
}

fn dash(value: Option<String>) -> String {
    value.unwrap_or_else(|| "-".into())
}

/// What a node holds, in the words of the form that sets one up.
pub(super) fn keeps_text(cx: &Cx, keeps: Keeps) -> String {
    match keeps {
        Keeps::Everything => cx.say("local.keep_all").to_string(),
        Keeps::Recent => cx.say("local.keep_recent").to_string(),
        Keeps::Headers => cx.say("local.keep_headers").to_string(),
        Keeps::Unsaid => "-".to_string(),
    }
}

/// Every device around, for a person who does not need the expert's table.
fn plain_rows(ui: &mut Ui, cx: &Cx, entries: &[&Entry]) {
    let columns = Columns::new(ui, &PLAIN_WEIGHTS);
    columns.row(ui, |ui, index| {
        let keys = ["found.device", "found.keeps", "found.work"];
        header_cell(ui, cx.t, cx.say(keys[index]));
    });
    for entry in entries {
        columns.row(ui, |ui, index| match index {
            0 => cell(ui, device_text(cx, entry)),
            1 => cell(ui, keeps_text(cx, entry.keeps)),
            _ => work_cell(ui, cx, entry),
        });
    }
}

fn device_text(cx: &Cx, entry: &Entry) -> String {
    let device = format::device(cx.voice(), &entry.device);
    match entry.name.is_empty() {
        true => device,
        false => format!("{device}  {}", entry.name),
    }
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
        1 => cell(ui, device_text(cx, entry)),
        2 => cell(ui, keeps_text(cx, entry.keeps)),
        3 => cell(ui, dash(entry.height.map(|height| format::number(height.into())))),
        4 => cell(ui, dash(entry.work_done.map(format::number))),
        5 => cell(ui, dash(Some(entry.build.clone()).filter(|build| !build.is_empty()))),
        6 => work_cell(ui, cx, entry),
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
            let keys = ["pro.reach", "pro.device", "found.keeps", "pro.height", "pro.work", "pro.build", "pro.activity"];
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

/// Everything found but this app itself, on any coin.
fn other_devices<'a>(cx: &Cx<'a>) -> Vec<&'a Entry> {
    let chains = cx.snap.groups.iter().flat_map(|group| &group.chains);
    let entries = chains.flat_map(|chain| &chain.entries);
    entries.filter(|entry| entry.reach != Reach::ThisApp).collect()
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
        // Joining a split takes its rules, not those of the first one found.
        coin: others().find_map(|chain| chain.coin.as_ref()).unwrap_or(&identity.coin),
        count: around.len(),
        address: first.address.clone(),
        split: others().any(|chain| chain.relation == Relation::Split),
    })
}

impl Coins {
    fn this_coin(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        let node = &cx.snap.node;
        let mut back = false;
        card(ui, t, |ui| {
            ui.horizontal(|ui| {
                dot(ui, t.good);
                if let Some(coin) = &node.coin {
                    section(ui, &coin.name);
                    tag(ui, &coin.ticker, t.muted, t);
                    if !coin.forks.is_empty() {
                        tag(ui, cx.say("coins.split"), t.warn, t);
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if button(ui, t, cx.say("coins.save_file"), Kind::Quiet).clicked() {
                        let picker = Picker::save("coins.save_file", "coin", COIN_ENDING, cx.folder.as_deref());
                        self.filing = Some(Filing::Save(picker));
                    }
                    if let Some(coin) = node.coin.as_ref().filter(|_| cx.expert()) {
                        if button(ui, t, cx.say("pro.fork"), Kind::Quiet).clicked() {
                            self.fork = Some(ForkForm::new(coin, node.height));
                        }
                    }
                    if node.coin.as_ref().is_some_and(|coin| !coin.forks.is_empty())
                        && button(ui, t, cx.say("coins.fork_back"), Kind::Quiet).clicked()
                    {
                        back = true;
                    }
                });
            });
            meta(ui, &cx.fill("coins.this_line", &[&format::number(node.height.into()), &node.peers.to_string()]), t);
            if let Some(fork) = node.coin.as_ref().and_then(|coin| coin.forks.last()) {
                meta(ui, &cx.fill("coins.forked_line", &[&format::number(fork.from_height.into())]), t);
            }
        });
        if back {
            let name = node.coin.as_ref().map_or(String::new(), |coin| coin.name.clone());
            *cx.slip = Some(cx.slip_for(Slipping {
                what: cx.say("coins.fork_back"),
                rows: vec![(cx.say("coin.name"), name)],
                warning: cx.say("coins.fork_back_warning"),
                sign: cx.say("coins.fork_back"),
                danger: false,
                order: vec![Command::ForkBack],
            }));
        }
    }

    fn fork_sheet(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        let (Some(form), Some(coin)) = (&mut self.fork, &cx.snap.node.coin) else {
            return;
        };
        let height = cx.snap.node.height;
        let (slip, outside) = sheet(ui, t, "fork", tokens::SHEET_WIDE, |ui| {
            sheet_head(ui, cx.say("pro.fork"));
            ScrollArea::vertical().max_height(sheet_room(ui)).show(ui, |ui| form.show(ui, cx, coin, height));
            ui.add_space(tokens::GAP_WIDE);
            let order = form.order(coin, height);
            let mut next = None;
            let mut cancel = false;
            ui.horizontal(|ui| {
                let go = button_on(ui, t, cx.say("pro.fork_go"), Kind::Main, order.is_ok());
                if let Some(order) = order.ok().filter(|_| go.clicked()) {
                    next = Some(form.slip(cx, coin, height, order));
                }
                cancel = button(ui, t, cx.say("common.cancel"), Kind::Plain).clicked();
            });
            (next, cancel)
        });
        let (next, cancel) = slip;
        if let Some(next) = next {
            *cx.slip = Some(next);
        }
        if cancel || outside || cx.slip.is_some() {
            self.fork = None;
        }
    }

    fn found(&self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        let snap = cx.snap;
        let found: Vec<Found> = snap.groups.iter().filter_map(found_in).collect();
        let entries = other_devices(cx);
        if !entries.is_empty() && !cx.expert() {
            section(ui, cx.say("coins.devices"));
            plain_rows(ui, cx, &entries);
            ui.add_space(tokens::GAP_WIDE);
        }
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
            let hidden = !cx.expert() && !self.by_address;
            if hidden && button(ui, t, cx.say("coins.by_address"), Kind::Quiet).clicked() {
                self.by_address = true;
            }
        });
        // Up here, above the lists that grow and shrink, so the field stays put.
        self.add_address(ui, cx);
    }

    fn new_coin_sheet(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        let Some(form) = &mut self.new_coin else {
            return;
        };
        let (done, outside) = sheet(ui, t, "new-coin", tokens::SHEET_WIDE, |ui| {
            sheet_head(ui, cx.say("coins.new"));
            ScrollArea::vertical().max_height(sheet_room(ui)).show(ui, |ui| form.show(ui, cx));
            form.problem(ui, cx);
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
        self.this_coin(ui, cx);
        ui.add_space(tokens::GAP_WIDE);
        self.start_buttons(ui, cx);
        // What turns up on its own goes under the buttons, so nothing moves under a press.
        ui.add_space(tokens::GAP_WIDE);
        widgets::slot(ui, |ui| self.found(ui, cx));
        ui.add_space(tokens::GAP_WIDE);
        widgets::slot(ui, |ui| history_card(ui, cx));
        // Who is connected now, with what each connection carried.
        ui.add_space(tokens::GAP_WIDE);
        widgets::slot(ui, |ui| self.peers(ui, cx));
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
        self.fork_sheet(ui, cx);
        self.file_sheet(ui, cx);
    }
}

/// How tall the rows of a sheet can stand with its title and buttons still on the screen.
fn sheet_room(ui: &Ui) -> f32 {
    ui.ctx().content_rect().height() - SHEET_FRAME
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
