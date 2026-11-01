//! Nodes on this computer. Each is a real process with its own folder and wallet or a shared one,
//! held to numbers the operating system enforces, and shown beside what it really uses.
use super::{Cx, Page};
use crate::core::Command;
use crate::local::{Enforcement, LocalView, NodeSpec, Preset, WalletChoice, PROCESS_OVERHEAD_MIB};
use crate::ui::format;
use crate::ui::tokens;
use crate::ui::widgets::{
    self, button, card, chip, dot, field, header_cell, meta, number_field, section,
    segments, strong_text, switch_row, Columns, Kind,
};
use crate::ui::Icon;
use egui::Ui;

const MIB: u64 = 1024 * 1024;

pub struct Local {
    form: NodeSpec,
    open_form: bool,
    pay_to: String,
    mine_to: String,
    pay_amount: String,
    payer: Option<u32>,
}

impl Default for Local {
    fn default() -> Self {
        Self {
            form: Preset::Esp32S3.spec(0),
            open_form: false,
            pay_to: String::new(),
            mine_to: String::new(),
            pay_amount: String::new(),
            payer: None,
        }
    }
}

fn next_id(cx: &Cx) -> u32 {
    let highest = cx.snap.local.iter().map(|view| view.spec.id).max();
    highest.map_or(0, |id| id + 1)
}

fn preset_name(cx: &Cx, preset: Preset) -> &'static str {
    cx.say(match preset {
        Preset::Esp32S3 => "local.preset_esp32",
        Preset::RaspberryPi4 => "local.preset_pi",
        Preset::Custom => "local.preset_custom",
    })
}

fn given_line(cx: &Cx, index: usize, spec: &NodeSpec, given: &str) -> String {
    let none = (index == 0 && spec.hash_cap == 0) || (index == 2 && spec.cpu_percent == 0);
    match none {
        true => given.to_string(),
        false => cx.fill("local.given", &[given]),
    }
}

/// Given and used, one line each, in the same columns.
fn budget(ui: &mut Ui, cx: &Cx, view: &LocalView) {
    let t = cx.t;
    let spec = &view.spec;
    let status = view.status.as_ref();
    let measured = view.measured;
    let columns = Columns::new(ui, &[1.2, 1.2, 1.2, 1.2]);
    let cap = match spec.hash_cap {
        0 => cx.say("mining.no_cap").to_string(),
        cap => format::rate(cap),
    };
    let share = match spec.cpu_percent {
        0 => cx.say("local.no_limit").to_string(),
        percent => format!("{percent}%"),
    };
    let given = [
        cap,
        cx.fill(
            "local.memory_given",
            &[
                &format::mebibytes(spec.memory_mib * MIB),
                &format::mebibytes(PROCESS_OVERHEAD_MIB * MIB),
            ],
        ),
        share,
        format::mebibytes(spec.disk_mib * MIB),
    ];
    let unknown = || "-".to_string();
    let used = [
        status.map_or_else(unknown, |status| format::rate(status.hashrate)),
        measured.map_or_else(unknown, |used| {
            format::mebibytes(used.cgroup_bytes.unwrap_or(0).max(used.rss_bytes))
        }),
        measured.map_or_else(unknown, |used| format!("{:.0}%", used.cpu_percent)),
        measured.map_or_else(unknown, |used| format::mebibytes(used.disk_bytes)),
    ];
    let keys = ["local.hash", "local.memory", "local.cpu", "local.disk"];
    columns.row(ui, |ui, index| header_cell(ui, t, cx.say(keys[index])));
    columns.row(ui, |ui, index| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.label(strong_text(used[index].clone()));
            meta(ui, &given_line(cx, index, spec, &given[index]), t);
        });
    });
}

impl Local {
    fn node_card(&mut self, ui: &mut Ui, cx: &Cx, view: &LocalView) {
        let t = cx.t;
        let id = view.spec.id;
        let running = view.pid.is_some();
        card(ui, t, |ui| {
            ui.horizontal(|ui| {
                dot(ui, if running { t.good } else { t.muted });
                section(ui, &view.spec.name);
                chip(ui, preset_name(cx, view.spec.preset), t.muted, t);
                let wallet = match view.spec.wallet {
                    WalletChoice::Own => "local.wallet_own",
                    WalletChoice::Shared => "local.wallet_shared",
                };
                chip(ui, cx.say(wallet), t.muted, t);
                if let Some(ended) = &view.ended {
                    chip(ui, &cx.fill("local.ended", &[ended]), t.bad, t);
                }
                if view.status.as_ref().is_some_and(|status| status.storage_full) {
                    chip(ui, cx.say("local.disk_full"), t.warn, t);
                }
            });
            if let Some(status) = &view.status {
                let ticker = cx.snap.node.coin.as_ref().map_or("", |coin| coin.ticker.as_str());
                let line = cx.fill(
                    "local.line",
                    &[
                        &format::number(status.height.into()),
                        &status.peers.to_string(),
                        &format::coins(status.balance, ticker),
                        &format::number(status.blocks_mined.into()),
                    ],
                );
                meta(ui, &line, t);
            }
            budget(ui, cx, view);
            ui.horizontal(|ui| {
                if running {
                    if button(ui, t, cx.say("local.stop"), Kind::Plain).clicked() {
                        cx.send(Command::StopNode(id));
                    }
                    let mining = view.status.as_ref().is_some_and(|status| status.mining);
                    let key = if mining { "boards.stop_mining" } else { "boards.start_mining" };
                    if button(ui, t, cx.say(key), Kind::Plain).clicked() {
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
                        cx.send(Command::RemoveNode(id));
                    }
                }
            });
            if self.payer == Some(id) {
                self.pay_form(ui, cx, id);
            }
        });
    }

    fn pay_form(&mut self, ui: &mut Ui, cx: &Cx, id: u32) {
        let t = cx.t;
        ui.horizontal_wrapped(|ui| {
            field(ui, &mut self.pay_to, cx.say("wallet.to"), 340.0);
            field(ui, &mut self.pay_amount, cx.say("wallet.amount"), 110.0);
            if button(ui, t, cx.say("wallet.send"), Kind::Main).clicked() {
                cx.send(Command::NodePay {
                    id,
                    to: self.pay_to.trim().to_string(),
                    amount: std::mem::take(&mut self.pay_amount),
                });
            }
        });
        ui.horizontal_wrapped(|ui| {
            field(ui, &mut self.mine_to, cx.say("mining.pay_to_hint"), 340.0);
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
            let columns = Columns::new(ui, &[1.0, 1.0, 1.0, 1.0, 1.0]);
            let form = &mut self.form;
            let names = [
                "local.name",
                "local.hash_cap",
                "local.memory_mib",
                "local.cpu_percent",
                "local.disk_mib",
            ];
            columns.row(ui, |ui, index| header_cell(ui, t, cx.say(names[index])));
            columns.row(ui, |ui, index| {
                let width = ui.available_width() - tokens::GAP;
                let mut percent = u64::from(form.cpu_percent);
                match index {
                    0 => {
                        field(ui, &mut form.name, "", width);
                    }
                    1 => {
                        number_field(ui, &mut form.hash_cap, width);
                    }
                    2 => {
                        number_field(ui, &mut form.memory_mib, width);
                    }
                    3 => {
                        if number_field(ui, &mut percent, width) {
                            form.cpu_percent = percent as u32;
                        }
                    }
                    _ => {
                        number_field(ui, &mut form.disk_mib, width);
                    }
                }
            });
            ui.horizontal(|ui| {
                let items = [
                    (WalletChoice::Own, cx.say("local.wallet_own").to_string()),
                    (WalletChoice::Shared, cx.say("local.wallet_shared").to_string()),
                ];
                if let Some(wallet) = segments(ui, t, &items, Some(form.wallet)) {
                    form.wallet = wallet;
                }
                ui.add_space(tokens::GAP_WIDE);
                widgets::switch_row(ui, t, &mut form.joined, cx.say("local.joined"));
                switch_row(ui, t, &mut form.mining, cx.say("local.mine"));
                switch_row(ui, t, &mut form.listen_lan, cx.say("local.listen_lan"));
            });
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
                if button(ui, t, cx.say("local.cancel"), Kind::Quiet).clicked() {
                    self.open_form = false;
                }
            });
        });
    }
}

fn enforcement_line(cx: &Cx) -> Option<&'static str> {
    let view = cx.snap.local.first()?;
    Some(cx.say(match view.enforcement {
        Enforcement::Cgroup => "local.held_systemd",
        Enforcement::AddressSpace => "local.held_rlimit",
        Enforcement::JobObject => "local.held_job",
        Enforcement::Priority => "local.held_priority",
        Enforcement::Nothing => "local.held_nothing",
    }))
}

impl Page for Local {
    fn title(&self) -> &'static str {
        "page.local"
    }

    fn icon(&self) -> Icon {
        Icon::Nodes
    }

    fn show(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        widgets::title(ui, cx.say("page.local"));
        if self.open_form {
            self.add_form(ui, cx);
            ui.add_space(tokens::GAP);
        } else if button(ui, t, cx.say("local.new_node"), Kind::Main).clicked() {
            self.form.name = format!("node-{}", next_id(cx));
            self.open_form = true;
        }
        if cx.snap.local.is_empty() {
            ui.add_space(tokens::GAP);
            widgets::empty(ui, t, cx.say("local.none"), cx.say("local.none_body"));
            return;
        }
        if let Some(line) = enforcement_line(cx) {
            meta(ui, line, t);
        }
        for view in &cx.snap.local {
            ui.add_space(tokens::GAP);
            self.node_card(ui, cx, view);
        }
    }
}
