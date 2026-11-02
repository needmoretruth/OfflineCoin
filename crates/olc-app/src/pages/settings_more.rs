//! More of the register: what mining pays to, which link comes first, how much to keep, and
//! who can find this computer.
use super::coins::medium_text;
use super::settings::switch_setting;
use super::Cx;
use crate::core::radios::NoBluetooth;
use crate::core::{Command, Personal};
use crate::local::Holding;
use crate::ui::format;
use crate::ui::register::{group, row};
use crate::ui::widgets::{self, button, field, meta, number_field, segments, switch, Kind};
use egui::Ui;
use olc_core::FEWEST_RECENT;

#[derive(Default)]
pub struct ExtraValues {
    miner: String,
    note: String,
    ports: String,
    card: String,
    recent: u64,
}

pub struct Extras<'a> {
    pub values: &'a mut ExtraValues,
}

fn link_rows(ui: &mut Ui, cx: &Cx) {
    let t = cx.t;
    let rank = cx.snap.node.links;
    group(ui, cx.say("settings.links"));
    for (place, medium) in rank.0.into_iter().enumerate() {
        let name = format!("{}. {}", place + 1, medium_text(cx, medium));
        let about = if place == 0 { cx.say("settings.links_first") } else { "" };
        row(ui, t, &name, about, |ui| {
            if place > 0 {
                let label = cx.fill("settings.link_up", &[medium_text(cx, medium)]);
                if button(ui, t, &label, Kind::Plain).clicked() {
                    let mut order = rank;
                    order.0.swap(place, place - 1);
                    cx.send(Command::SetLinks(order));
                }
            }
        });
    }
    let bluetooth = cx.snap.node.bluetooth;
    let about = cx.say(match cx.snap.node.no_bluetooth {
        None => "settings.bluetooth_note",
        Some(NoBluetooth::Adapter) => "settings.no_bluetooth",
        Some(NoBluetooth::Build) => "settings.no_bluetooth_build",
        Some(NoBluetooth::System) => "settings.no_bluetooth_system",
    });
    row(ui, t, cx.say("boards.bluetooth"), about, |ui| {
        widgets::slot(ui, |ui| {
            if let Some(mut on) = bluetooth {
                if switch(ui, t, &mut on, cx.say("boards.bluetooth")).clicked() {
                    cx.send(Command::Own(Personal::SetBluetooth(on)));
                }
            }
        });
    });
}

impl Extras<'_> {
    fn mining(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        let keys = ("settings.mine_on_start", "settings.mine_on_start_about");
        switch_setting(ui, cx, keys, |settings| &mut settings.mine_on_start);
        let node = &cx.snap.node;
        let goes_to = match node.miner_is_chosen {
            true => format::short_address(&node.miner),
            false => cx.say("settings.miner_me").to_string(),
        };
        row(ui, t, cx.say("settings.miner"), &goes_to, |ui| {
            field(ui, &mut self.values.miner, cx.say("send.to_hint"), 170.0);
            if button(ui, t, cx.say("common.save"), Kind::Plain).clicked() {
                let to = std::mem::take(&mut self.values.miner);
                cx.send(Command::Own(Personal::SetMiner(to)));
            }
        });
        row(ui, t, cx.say("settings.mine_note"), cx.say("settings.mine_note_about"), |ui| {
            if self.values.note.is_empty() {
                self.values.note.clone_from(&node.mine_note);
            }
            field(ui, &mut self.values.note, cx.say("settings.mine_note"), 170.0);
            if button(ui, t, cx.say("common.save"), Kind::Plain).clicked() {
                cx.send(Command::Own(Personal::SetMineNote(self.values.note.clone())));
            }
        });
        let keys = ("settings.auto_update", "settings.auto_update_about");
        switch_setting(ui, cx, keys, |settings| &mut settings.auto_update);
    }

    fn keeping(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        let node = &cx.snap.node;
        group(ui, cx.say("settings.keeping"));
        let held = match (&node.light, node.fits) {
            (Some(_), _) => cx.say("storage.nothing_held").to_string(),
            (None, Some(fits)) => cx.fill(
                "storage.held_fits",
                &[&format::mebibytes(node.stored_bytes), &format::number(fits.into())],
            ),
            (None, None) => cx.fill("storage.held", &[&format::mebibytes(node.stored_bytes)]),
        };
        let holding = match (&node.light, node.recent) {
            (Some(_), _) => Holding::Headers,
            (None, Some(_)) => Holding::Recent,
            (None, None) => Holding::Everything,
        };
        row(ui, t, cx.say("settings.keep"), &held, |ui| {
            let items = [Holding::Everything, Holding::Recent, Holding::Headers]
                .map(|holding| (holding, cx.say(holding.key()).to_string()));
            if let Some(chosen) = segments(ui, t, &items, Some(holding)).filter(|c| *c != holding) {
                if chosen != Holding::Headers {
                    let recent = (chosen == Holding::Recent).then_some(self.values.recent.max(FEWEST_RECENT));
                    cx.send(Command::Own(Personal::SetRecent(recent)));
                }
                cx.send(Command::Own(Personal::SetLight(chosen == Holding::Headers)));
            }
        });
        if holding == Holding::Headers {
            meta(ui, cx.say("local.light_trust"), t);
        }
        if holding == Holding::Recent {
            if self.values.recent == 0 {
                self.values.recent = node.recent.unwrap_or(FEWEST_RECENT);
            }
            let about = format!("{} {}", cx.say("storage.blocks_about"), cx.say("storage.headers"));
            row(ui, t, cx.say("storage.blocks"), &about, |ui| {
                number_field(ui, &mut self.values.recent, cx.say("storage.blocks"), 120.0);
                if button(ui, t, cx.say("common.save"), Kind::Plain).clicked() {
                    cx.send(Command::Own(Personal::SetRecent(Some(self.values.recent))));
                }
            });
        }
        row(ui, t, cx.say("storage.merge"), cx.say("storage.merge_about"), |ui| {
            let mut merging = node.merging;
            if switch(ui, t, &mut merging, cx.say("storage.merge")).clicked() {
                cx.send(Command::Own(Personal::SetMerging(merging)));
            }
        });
        if cx.expert() {
            row(ui, t, cx.say("pro.version_one"), cx.say("pro.version_one_about"), |ui| {
                let mut on = node.version_one;
                if switch(ui, t, &mut on, cx.say("pro.version_one")).clicked() {
                    cx.send(Command::Own(Personal::SetVersionOne(on)));
                }
            });
        }
        widgets::slot(ui, |ui| {
            if node.rebuilt {
                meta(ui, cx.say("storage.rebuilt"), t);
            }
        });
    }

    fn finding(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        group(ui, cx.say("settings.finding"));
        let keys = ("settings.listen_lan", "settings.listen_lan_about");
        switch_setting(ui, cx, keys, |settings| &mut settings.listen_lan);
        let keys = ("settings.scan_lan", "settings.scan_lan_about");
        switch_setting(ui, cx, keys, |settings| &mut settings.scan_lan);
        if !cx.expert() {
            return;
        }
        let keys = ("pro.broadcast", "pro.broadcast_about");
        switch_setting(ui, cx, keys, |settings| &mut settings.broadcast);
        row(ui, t, cx.say("pro.port"), cx.say("pro.port_about"), |ui| {
            let mut port = u64::from(cx.settings.port);
            if number_field(ui, &mut port, cx.say("pro.port"), 90.0) {
                cx.settings.port = u16::try_from(port).unwrap_or(u16::MAX);
                cx.save();
            }
        });
        row(ui, t, cx.say("pro.scan_ports"), cx.say("pro.scan_ports_about"), |ui| {
            if self.values.ports.is_empty() {
                let ports: Vec<String> = cx.settings.scan_ports.iter().map(u16::to_string).collect();
                self.values.ports = ports.join(", ");
            }
            if field(ui, &mut self.values.ports, cx.say("pro.scan_ports"), 170.0).lost_focus() {
                let read = self.values.ports.split(',').filter_map(|word| word.trim().parse().ok());
                cx.settings.scan_ports = read.collect();
                cx.save();
            }
        });
        row(ui, t, cx.say("pro.esp_now_card"), cx.say("pro.esp_now_card_about"), |ui| {
            field(ui, &mut self.values.card, cx.say("pro.esp_now_card"), 170.0);
            if button(ui, t, cx.say("common.save"), Kind::Plain).clicked() {
                cx.send(Command::Own(Personal::SetEspNowCard(self.values.card.trim().to_string())));
            }
        });
    }

    pub fn show(&mut self, ui: &mut Ui, cx: &mut Cx) {
        self.mining(ui, cx);
        link_rows(ui, cx);
        self.keeping(ui, cx);
        self.finding(ui, cx);
    }
}
