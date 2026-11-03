//! Nodes on this computer. Each is a real process with its own folder and wallet or a shared one,
//! held to numbers the operating system enforces, and shown beside what it really uses.
use super::{Cx, Slipping};
use crate::core::Command;
use crate::local::{
    Enforcement, Holding, LocalView, NodeSpec, Preset, WalletChoice, PROCESS_OVERHEAD_MIB,
};
use crate::ui::format;
use crate::ui::tokens;
use crate::ui::widgets::{
    self, button, button_on, card, dot, field, header_cell, meta, number_field, section, segments,
    strong_text,
    switch_row, tag, Columns, Kind,
};
use egui::Ui;
use std::collections::HashMap;

const MIB: u64 = 1024 * 1024;

pub struct Local {
    form: NodeSpec,
    open_form: bool,
    pay_to: String,
    mine_to: String,
    pay_amount: String,
    payer: Option<u32>,
    caps: HashMap<u32, u64>,
}

impl Default for Local {
    fn default() -> Self {
        Self {
            form: Preset::default().spec(0),
            open_form: false,
            pay_to: String::new(),
            mine_to: String::new(),
            pay_amount: String::new(),
            payer: None,
            caps: HashMap::new(),
        }
    }
}

fn next_id(cx: &Cx) -> u32 {
    let highest = cx.snap.local.iter().map(|view| view.spec.id).max();
    highest.map_or(0, |id| id + 1)
}

fn preset_name(cx: &Cx, preset: Preset) -> &'static str {
    cx.say(match preset {
        Preset::Computer => "local.preset_computer",
        Preset::Esp32S3 => "local.preset_esp32",
        Preset::RaspberryPi4 => "local.preset_pi",
        Preset::Custom => "local.preset_custom",
    })
}

/// What the node may use and what it uses. A node that cannot mine has no speed to speak of.
fn budget_items(cx: &Cx, view: &LocalView) -> Vec<(&'static str, String, String)> {
    let spec = &view.spec;
    let voice = cx.voice();
    let measured = view.measured;
    let unknown = || "-".to_string();
    let given = |text: String| cx.fill("local.given", &[&text]);
    let mut items = Vec::new();
    if spec.holding != Holding::Headers {
        let cap = match spec.hash_cap {
            0 => cx.say("mining.no_cap").to_string(),
            cap => given(format::rate(voice, cap)),
        };
        let used = view.status.as_ref().map_or_else(unknown, |status| format::rate(voice, status.hashrate));
        items.push(("local.hash", used, cap));
    }
    let memory = cx.fill(
        "local.memory_given",
        &[&format::mebibytes(spec.memory_mib * MIB), &format::mebibytes(PROCESS_OVERHEAD_MIB * MIB)],
    );
    let bytes = |used: crate::local::Measured| used.cgroup_bytes.unwrap_or(used.rss_bytes);
    items.push(("local.memory", measured.map_or_else(unknown, |used| format::mebibytes(bytes(used))), given(memory)));
    let share = match spec.cpu_percent {
        0 => cx.say("local.no_limit").to_string(),
        percent => given(format!("{percent}%")),
    };
    items.push(("local.cpu", measured.map_or_else(unknown, |used| format!("{:.0}%", used.cpu_percent)), share));
    let disk = given(format::mebibytes(spec.disk_mib * MIB));
    items.push(("local.disk", measured.map_or_else(unknown, |used| format::mebibytes(used.disk_bytes)), disk));
    items
}

/// Given and used, in the same columns.
fn budget(ui: &mut Ui, cx: &Cx, view: &LocalView) {
    let t = cx.t;
    let items = budget_items(cx, view);
    let columns = Columns::new(ui, &vec![1.2; items.len()]);
    columns.row(ui, |ui, index| header_cell(ui, t, cx.say(items[index].0)));
    columns.row(ui, |ui, index| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.label(strong_text(items[index].1.clone()));
            meta(ui, &items[index].2, t);
        });
    });
}

fn tags(ui: &mut Ui, cx: &Cx, view: &LocalView) {
    let t = cx.t;
    // A miner of this computer needs no tag for it. One that copies a board says which.
    if view.spec.preset.is_board() {
        tag(ui, preset_name(cx, view.spec.preset), t.muted, t);
    }
    let wallet = match view.spec.wallet {
        WalletChoice::Own => "local.wallet_own",
        WalletChoice::Shared => "local.wallet_shared",
    };
    tag(ui, cx.say(wallet), t.muted, t);
    tag(ui, cx.say(view.spec.holding.key()), t.muted, t);
    if view.ended.is_some() {
        tag(ui, cx.say("local.ended"), t.bad, t);
    }
    if view.status.as_ref().is_some_and(|status| status.storage_full) {
        tag(ui, cx.say("local.disk_full"), t.warn, t);
    }
}

/// Why a node stopped, in the system's words for an expert.
fn ended_line(ui: &mut Ui, cx: &Cx, view: &LocalView) {
    if let Some(how) = &view.ended {
        meta(ui, if cx.expert() { how } else { cx.say("local.ended_about") }, cx.t);
    }
}

fn status_line(ui: &mut Ui, cx: &Cx, view: &LocalView) {
    let Some(status) = &view.status else {
        return;
    };
    let (key, last) = match status.headers {
        Some(headers) => ("local.line_light", headers),
        None => ("local.line", status.blocks_mined),
    };
    let line = cx.fill(
        key,
        &[
            &format::number(status.height.into()),
            &status.peers.to_string(),
            &format::coins(status.balance, cx.ticker()),
            &format::number(last.into()),
        ],
    );
    meta(ui, &line, cx.t);
}

impl Local {
    /// A running node takes a new cap at once. A stopped one starts with it.
    fn cap_row(&mut self, ui: &mut Ui, cx: &Cx, view: &LocalView) {
        let t = cx.t;
        let id = view.spec.id;
        let name = cx.say("local.hash_cap");
        let cap = self.caps.entry(id).or_insert(view.spec.hash_cap);
        ui.horizontal(|ui| {
            meta(ui, name, t);
            number_field(ui, cap, name, 120.0);
            let changed = *cap != view.spec.hash_cap;
            if button_on(ui, t, cx.say("common.save"), Kind::Plain, changed).clicked() {
                cx.send(Command::NodeCap { id, cap: *cap });
            }
        });
    }

    fn node_card(&mut self, ui: &mut Ui, cx: &mut Cx, view: &LocalView) {
        let t = cx.t;
        let id = view.spec.id;
        let running = view.pid.is_some();
        let mut slip = None;
        card(ui, t, |ui| {
            ui.horizontal_wrapped(|ui| {
                dot(ui, if running { t.good } else { t.muted });
                section(ui, &view.spec.name);
                tags(ui, cx, view);
            });
            if view.spec.holding == Holding::Headers {
                meta(ui, cx.say("local.light_trust"), t);
            }
            status_line(ui, cx, view);
            ended_line(ui, cx, view);
            budget(ui, cx, view);
            if view.spec.holding != Holding::Headers {
                self.cap_row(ui, cx, view);
            }
            ui.horizontal(|ui| {
                if running {
                    if button(ui, t, cx.say("local.stop"), Kind::Plain).clicked() {
                        cx.send(Command::StopNode(id));
                    }
                    let mining = view.status.as_ref().is_some_and(|status| status.mining);
                    let key = if mining { "boards.stop_mining" } else { "boards.start_mining" };
                    let can_mine = view.spec.holding != Holding::Headers;
                    if can_mine && button(ui, t, cx.say(key), Kind::Plain).clicked() {
                        cx.send(Command::NodeMining { id, on: !mining });
                    }
                    if button(ui, t, cx.say("local.pay_from"), Kind::Plain).clicked() {
                        self.payer = if self.payer == Some(id) { None } else { Some(id) };
                    }
                } else {
                    if button(ui, t, cx.say("local.start"), Kind::Main).clicked() {
                        cx.send(Command::StartNode(id));
                    }
                    if button(ui, t, cx.say("local.remove"), Kind::Danger).clicked() {
                        slip = Some(cx.slip_for(Slipping {
                            what: cx.say("local.remove"),
                            rows: vec![(cx.say("local.name"), view.spec.name.clone())],
                            warning: cx.say("local.remove_warning"),
                            sign: cx.say("local.remove"),
                            danger: true,
                            order: vec![Command::RemoveNode(id)],
                        }));
                    }
                }
            });
            if self.payer == Some(id) {
                self.pay_form(ui, cx, id);
            }
            if running && view.spec.holding != Holding::Headers { super::pool_join::rows(ui, cx, crate::core::Who::Node(id)) }
        });
        if slip.is_some() {
            *cx.slip = slip;
        }
    }

    fn pay_form(&mut self, ui: &mut Ui, cx: &Cx, id: u32) {
        let t = cx.t;
        ui.horizontal_wrapped(|ui| {
            field(ui, &mut self.pay_to, cx.say("send.to_hint"), 340.0);
            field(ui, &mut self.pay_amount, cx.say("send.amount_hint"), 110.0);
            if button(ui, t, cx.say("send.title"), Kind::Main).clicked() {
                cx.send(Command::NodePay {
                    id,
                    to: self.pay_to.trim().to_string(),
                    amount: std::mem::take(&mut self.pay_amount),
                });
            }
        });
        ui.horizontal_wrapped(|ui| {
            field(ui, &mut self.mine_to, cx.say("boards.mine_to_hint"), 340.0);
            if button(ui, t, cx.say("boards.mine_to"), Kind::Plain).clicked() {
                let to = self.mine_to.trim().to_string();
                cx.send(Command::NodeMiner { id, to });
            }
        });
        let others = cx.snap.local.iter().filter(|view| view.spec.id != id);
        ui.horizontal_wrapped(|ui| {
            for other in others {
                let Some(address) = other.status.as_ref().map(|status| status.address.clone()) else {
                    continue;
                };
                if button(ui, t, &other.spec.name, Kind::Quiet).clicked() {
                    self.pay_to = address;
                }
            }
        });
    }

    fn numbers(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        let form = &mut self.form;
        let mining = form.holding != Holding::Headers;
        let names: Vec<&str> = [
            Some("local.name"),
            mining.then_some("local.hash_cap"),
            Some("local.memory_mib"),
            Some("local.cpu_percent"),
            Some("local.disk_mib"),
        ]
        .into_iter()
        .flatten()
        .collect();
        let columns = Columns::new(ui, &vec![1.0; names.len()]);
        columns.row(ui, |ui, index| header_cell(ui, t, cx.say(names[index])));
        columns.row(ui, |ui, index| {
            let width = ui.available_width() - tokens::GAP;
            let name = cx.say(names[index]);
            let mut percent = u64::from(form.cpu_percent);
            match names[index] {
                "local.name" => {
                    field(ui, &mut form.name, name, width);
                }
                "local.hash_cap" => {
                    number_field(ui, &mut form.hash_cap, name, width);
                }
                "local.memory_mib" => {
                    number_field(ui, &mut form.memory_mib, name, width);
                }
                "local.cpu_percent" => {
                    if number_field(ui, &mut percent, name, width) {
                        form.cpu_percent = u32::try_from(percent).unwrap_or(0);
                    }
                }
                _ => {
                    number_field(ui, &mut form.disk_mib, name, width);
                }
            }
        });
    }

    fn choices(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        let form = &mut self.form;
        ui.horizontal_wrapped(|ui| {
            let items = [
                (WalletChoice::Own, cx.say("local.wallet_own").to_string()),
                (WalletChoice::Shared, cx.say("local.wallet_shared").to_string()),
            ];
            if let Some(wallet) = segments(ui, t, &items, Some(form.wallet)) {
                form.wallet = wallet;
            }
            ui.add_space(tokens::GAP_WIDE);
            switch_row(ui, t, &mut form.joined, cx.say("local.joined"));
            if form.holding != Holding::Headers {
                switch_row(ui, t, &mut form.mining, cx.say("local.mine"));
            }
            switch_row(ui, t, &mut form.listen_lan, cx.say("local.listen_lan"));
        });
        ui.horizontal(|ui| {
            let items = [Holding::Everything, Holding::Recent, Holding::Headers]
                .map(|holding| (holding, cx.say(holding.key()).to_string()));
            if let Some(holding) = segments(ui, t, &items, Some(form.holding)) {
                form.holding = holding;
                form.mining &= holding != Holding::Headers;
            }
            if form.holding == Holding::Recent {
                ui.add_space(tokens::GAP_WIDE);
                number_field(ui, &mut form.recent, cx.say("storage.blocks"), 100.0);
            }
        });
        if form.holding == Holding::Headers {
            meta(ui, cx.say("local.light_trust"), t);
        }
    }

    fn add_form(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        card(ui, t, |ui| {
            section(ui, cx.say("local.new_node"));
            let presets: Vec<(Preset, String)> = Preset::ALL
                .iter()
                .map(|preset| (*preset, preset_name(cx, *preset).to_string()))
                .collect();
            if let Some(preset) = segments(ui, t, &presets, Some(self.form.preset)) {
                let name = self.form.name.clone();
                self.form = preset.spec(0);
                self.form.name = name;
            }
            if self.form.preset == Preset::RaspberryPi4 {
                meta(ui, cx.say("local.pi_note"), t);
            }
            if cx.expert() {
                self.numbers(ui, cx);
                self.choices(ui, cx);
            }
            ui.horizontal(|ui| {
                if button(ui, t, cx.say("local.add"), Kind::Main).clicked() {
                    let id = next_id(cx);
                    let mut spec = self.form.clone();
                    spec.id = id;
                    if spec.name.trim().is_empty() || spec.name.starts_with("node-") {
                        spec.name = format!("node-{id}");
                    }
                    cx.send(Command::AddNode(Box::new(spec)));
                    self.open_form = false;
                }
                if button(ui, t, cx.say("common.cancel"), Kind::Quiet).clicked() {
                    self.open_form = false;
                }
            });
        });
    }

    pub fn show(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        if self.open_form {
            self.add_form(ui, cx);
            ui.add_space(tokens::GAP);
        }
        if cx.snap.local.is_empty() {
            if !self.open_form {
                let add = cx.say("local.new_node");
                if widgets::empty(ui, t, cx.say("local.none"), cx.say("local.none_body"), Some(add)) {
                    self.form.name = format!("node-{}", next_id(cx));
                    self.open_form = true;
                }
            }
            return;
        }
        if !self.open_form && button(ui, t, cx.say("local.new_node"), Kind::Plain).clicked() {
            self.form.name = format!("node-{}", next_id(cx));
            self.open_form = true;
        }
        widgets::slot(ui, |ui| {
            if let Some(line) = enforcement_line(cx) {
                meta(ui, line, t);
            }
        });
        let snap = cx.snap;
        for view in &snap.local {
            ui.add_space(tokens::GAP);
            self.node_card(ui, cx, view);
        }
    }
}

fn enforcement_line(cx: &Cx) -> Option<&'static str> {
    let view = cx.snap.local.first().filter(|_| cx.expert())?;
    Some(cx.say(match view.enforcement {
        Enforcement::Cgroup => "pro.held_systemd",
        Enforcement::AddressSpace => "pro.held_rlimit",
        Enforcement::JobObject => "pro.held_job",
        Enforcement::Priority => "pro.held_priority",
        Enforcement::Nothing => "pro.held_nothing",
    }))
}
