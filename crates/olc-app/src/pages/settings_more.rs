//! More of Settings: the Mining and Network groups, and how much of the chain to keep, which
//! sits with the General rows.
use super::coins::medium_text;
use super::settings::{speed_rows, switch_setting, when};
use super::settings_compare;
use super::Cx;
use crate::core::radios::NoBluetooth;
use crate::core::{Command, Personal};
use crate::local::Holding;
use crate::ui::format;
use crate::ui::register::{group, row, setting, setting_details, When};
use crate::ui::widgets::{self, button, field, meta, number_field, segments, switch, Kind};
use egui::{Label, Ui};
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

/// The links in the order boards try them, each but the first with a way up.
/// The width of a connection's name in the order, the longest with its number.
const LINK_NAME: f32 = 130.0;

fn links_row(ui: &mut Ui, cx: &Cx) {
    let t = cx.t;
    let rank = cx.snap.node.links;
    setting(ui, t, cx.say("settings.links"), when(cx, When::Now), cx.say("settings.links_first"), |ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for (place, medium) in rank.0.into_iter().enumerate() {
                ui.horizontal(|ui| {
                    // The names take one width, so every Move up stands in one column.
                    let room = egui::vec2(LINK_NAME, ui.spacing().interact_size.y);
                    ui.allocate_ui_with_layout(room, egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.set_width(LINK_NAME);
                        ui.add(Label::new(format!("{}. {}", place + 1, medium_text(cx, medium))).selectable(false));
                    });
                    if place > 0 && button(ui, t, cx.say("settings.up"), Kind::Quiet).clicked() {
                        let mut order = rank;
                        order.0.swap(place, place - 1);
                        cx.send(Command::SetLinks(order));
                    }
                });
            }
        });
    });
}

fn bluetooth_row(ui: &mut Ui, cx: &Cx) {
    let t = cx.t;
    let bluetooth = cx.snap.node.bluetooth;
    // With nothing to switch, the control column says why, as other rows show their value there.
    let Some(mut on) = bluetooth else {
        let state = cx.say(match cx.snap.node.no_bluetooth {
            Some(NoBluetooth::Build) => "settings.bt_build",
            Some(NoBluetooth::System) => "settings.bt_system",
            Some(NoBluetooth::Adapter) | None => "settings.bt_adapter",
        });
        row(ui, t, cx.say("boards.bluetooth"), cx.say("settings.bluetooth_about"), |ui| {
            ui.add(Label::new(egui::RichText::new(state).color(t.muted)).selectable(false));
        });
        return;
    };
    setting(ui, t, cx.say("boards.bluetooth"), when(cx, When::Now), cx.say("settings.bluetooth_note"), |ui| {
        if switch(ui, t, &mut on, cx.say("boards.bluetooth")).clicked() {
            cx.send(Command::Own(Personal::SetBluetooth(on)));
        }
    });
}

impl Extras<'_> {
    pub fn mining(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        group(ui, cx.say("settings.mining"));
        speed_rows(ui, cx);
        let keys = ("settings.mine_on_start", "settings.mine_on_start_about");
        // The console's `mining on|off` changes this in the core. The core's value is taken when
        // it moves, so a click here is not undone before the core has heard it.
        let held = cx.snap.mine_on_start;
        let id = ui.make_persistent_id("mine_on_start_seen");
        let seen: Option<bool> = ui.data(|data| data.get_temp(id));
        if seen != Some(held) {
            ui.data_mut(|data| data.insert_temp(id, held));
            cx.settings.mine_on_start = held;
        }
        switch_setting(ui, cx, keys, When::NextStart, |settings| &mut settings.mine_on_start);
        let now = when(cx, When::Now);
        let node = &cx.snap.node;
        let goes_to = match node.miner_is_chosen {
            true => format::short_address(&node.miner),
            false => cx.say("settings.miner_me").to_string(),
        };
        setting(ui, t, cx.say("settings.miner"), now, &goes_to, |ui| {
            field(ui, &mut self.values.miner, cx.say("send.to_hint"), 170.0);
            if button(ui, t, cx.say("common.save"), Kind::Plain).clicked() {
                let to = std::mem::take(&mut self.values.miner);
                cx.send(Command::Own(Personal::SetMiner(to)));
            }
        });
        setting(ui, t, cx.say("settings.mine_note"), now, cx.say("settings.mine_note_about"), |ui| {
            if self.values.note.is_empty() {
                self.values.note.clone_from(&node.mine_note);
            }
            field(ui, &mut self.values.note, cx.say("settings.mine_note"), 170.0);
            if button(ui, t, cx.say("common.save"), Kind::Plain).clicked() {
                cx.send(Command::Own(Personal::SetMineNote(self.values.note.clone())));
            }
        });
    }

    /// Full, recent or summaries only, with the three compared behind Details, then merging.
    pub fn keeping(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        let now = when(cx, When::Now);
        let node = &cx.snap.node;
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
        let items = [Holding::Everything, Holding::Recent, Holding::Headers]
            .map(|holding| (holding, cx.say(holding.key()).to_string()));
        let labels = (cx.say("explain.details"), cx.say("explain.hide"));
        let control = |ui: &mut Ui| {
            if let Some(chosen) = segments(ui, t, &items, Some(holding)).filter(|c| *c != holding) {
                if chosen != Holding::Headers {
                    let recent = (chosen == Holding::Recent).then_some(self.values.recent.max(FEWEST_RECENT));
                    cx.send(Command::Own(Personal::SetRecent(recent)));
                }
                cx.send(Command::Own(Personal::SetLight(chosen == Holding::Headers)));
            }
        };
        setting_details(ui, t, (cx.say("settings.keep"), now), (&held, "keeping", labels), control, |ui| {
            settings_compare::keeping(ui, cx, node.recent);
        });
        if holding == Holding::Headers {
            meta(ui, cx.say("local.light_trust"), t);
        }
        if holding == Holding::Recent {
            if self.values.recent == 0 {
                self.values.recent = node.recent.unwrap_or(FEWEST_RECENT);
            }
            let about = format!("{} {}", cx.say("storage.blocks_about"), cx.say("storage.headers"));
            setting(ui, t, cx.say("storage.blocks"), now, &about, |ui| {
                number_field(ui, &mut self.values.recent, cx.say("storage.blocks"), 120.0);
                if button(ui, t, cx.say("common.save"), Kind::Plain).clicked() {
                    cx.send(Command::Own(Personal::SetRecent(Some(self.values.recent))));
                }
            });
        }
        widgets::slot(ui, |ui| {
            if node.rebuilt {
                meta(ui, cx.say("storage.rebuilt"), t);
            }
        });
    }

    /// Who can find this computer and how it finds others, then the links boards use.
    pub fn network(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        group(ui, cx.say("settings.network"));
        let keys = ("settings.listen_lan", "settings.listen_lan_about");
        switch_setting(ui, cx, keys, When::Now, |settings| &mut settings.listen_lan);
        let keys = ("settings.scan_lan", "settings.scan_lan_about");
        switch_setting(ui, cx, keys, When::Now, |settings| &mut settings.scan_lan);
        links_row(ui, cx);
        bluetooth_row(ui, cx);
        if !cx.expert() {
            return;
        }
        let keys = ("pro.broadcast", "pro.broadcast_about");
        switch_setting(ui, cx, keys, When::Now, |settings| &mut settings.broadcast);
        // The port is taken once, when the app starts listening.
        setting(ui, t, cx.say("pro.port"), when(cx, When::NextStart), cx.say("pro.port_about"), |ui| {
            let mut port = u64::from(cx.settings.port);
            if number_field(ui, &mut port, cx.say("pro.port"), 90.0) {
                cx.settings.port = u16::try_from(port).unwrap_or(u16::MAX);
                cx.save();
            }
        });
        let now = when(cx, When::Now);
        setting(ui, t, cx.say("pro.scan_ports"), now, cx.say("pro.scan_ports_about"), |ui| {
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
        setting(ui, t, cx.say("pro.esp_now_card"), now, cx.say("pro.esp_now_card_about"), |ui| {
            field(ui, &mut self.values.card, cx.say("pro.esp_now_card"), 170.0);
            if button(ui, t, cx.say("common.save"), Kind::Plain).clicked() {
                cx.send(Command::Own(Personal::SetEspNowCard(self.values.card.trim().to_string())));
            }
        });
    }
}
