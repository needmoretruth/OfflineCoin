//! Every number and choice the app starts from.
use super::{Cx, Page};
use crate::core::{Command, Personal};
use crate::i18n::Lang;
use crate::settings::{Device, Theme};
use crate::ui::format;
use crate::ui::tokens;
use crate::ui::widgets::{
    self, button, card, field, meta, mono, number_field, section, segments, switch_row, Kind,
};
use crate::ui::Icon;
use egui::Ui;
use olc_core::FEWEST_RECENT;

pub fn device_name(device: Device) -> &'static str {
    match device {
        Device::Esp32 => "setup.esp32",
        Device::RaspberryPi => "setup.pi",
        Device::ThisComputer => "setup.this_computer",
        Device::ComputerAndPhone => "setup.computer_phone",
        Device::TwoComputers => "setup.two_computers",
    }
}

#[derive(Default)]
pub struct SettingsPage {
    ports: String,
    recent: u64,
}

fn labelled(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        let area = egui::vec2(180.0, tokens::CONTROL);
        let left = egui::Layout::left_to_right(egui::Align::Center);
        ui.allocate_ui_with_layout(area, left, |ui| {
            ui.set_width(area.x);
            ui.add(egui::Label::new(label).wrap());
        });
        add(ui);
    });
}

impl SettingsPage {
    fn look(cx: &mut Cx, ui: &mut Ui) {
        let t = cx.t;
        section(ui, cx.say("settings.look"));
        labelled(ui, cx.say("settings.language"), |ui| {
            let items = [(Lang::En, "English".to_string()), (Lang::Ko, "한국어".to_string())];
            if let Some(language) = segments(ui, t, &items, Some(cx.settings.language)) {
                cx.settings.language = language;
                cx.save();
            }
        });
        labelled(ui, cx.say("settings.theme"), |ui| {
            let items = [
                (Theme::System, cx.say("settings.theme_system").to_string()),
                (Theme::Light, cx.say("settings.theme_light").to_string()),
                (Theme::Dark, cx.say("settings.theme_dark").to_string()),
            ];
            if let Some(theme) = segments(ui, t, &items, Some(cx.settings.theme)) {
                cx.settings.theme = theme;
                cx.save();
            }
        });
        let label = cx.say("settings.reduce_motion");
        if switch_row(ui, t, &mut cx.settings.reduce_motion, label) {
            cx.save();
        }
    }

    fn mining(cx: &mut Cx, ui: &mut Ui) {
        let t = cx.t;
        section(ui, cx.say("settings.mining"));
        labelled(ui, cx.say("settings.hash_cap"), |ui| {
            if number_field(ui, &mut cx.settings.hash_cap, 120.0) {
                cx.save();
            }
            ui.label("H/s");
        });
        labelled(ui, cx.say("settings.board_cap"), |ui| {
            if number_field(ui, &mut cx.settings.board_cap, 120.0) {
                cx.save();
            }
            ui.label("H/s");
        });
        let label = cx.say("settings.mine_on_start");
        if switch_row(ui, t, &mut cx.settings.mine_on_start, label) {
            cx.save();
        }
        let label = cx.say("settings.auto_update");
        if switch_row(ui, t, &mut cx.settings.auto_update, label) {
            cx.save();
        }
        meta(ui, cx.say("settings.auto_update_note"), t);
    }

    fn network(&mut self, cx: &mut Cx, ui: &mut Ui) {
        let t = cx.t;
        section(ui, cx.say("settings.network"));
        labelled(ui, cx.say("settings.port"), |ui| {
            let mut port = u64::from(cx.settings.port);
            if number_field(ui, &mut port, 90.0) {
                cx.settings.port = port.min(u64::from(u16::MAX)) as u16;
                cx.save();
            }
        });
        let label = cx.say("settings.listen_lan");
        let lan = switch_row(ui, t, &mut cx.settings.listen_lan, label);
        let label = cx.say("settings.scan_lan");
        let scan = switch_row(ui, t, &mut cx.settings.scan_lan, label);
        let label = cx.say("settings.broadcast");
        if switch_row(ui, t, &mut cx.settings.broadcast, label) || lan || scan {
            cx.save();
        }
        labelled(ui, cx.say("settings.scan_ports"), |ui| {
            if self.ports.is_empty() {
                let ports: Vec<String> =
                    cx.settings.scan_ports.iter().map(u16::to_string).collect();
                self.ports = ports.join(", ");
            }
            if field(ui, &mut self.ports, "7000", 200.0).lost_focus() {
                let read = self.ports.split(',').filter_map(|word| word.trim().parse().ok());
                cx.settings.scan_ports = read.collect();
                cx.save();
            }
        });
    }
}

impl SettingsPage {
    /// How much of the chain this node keeps. Nothing about it changes the coin.
    fn storage(&mut self, cx: &Cx, ui: &mut Ui) {
        let t = cx.t;
        let node = &cx.snap.node;
        section(ui, cx.say("storage.title"));
        let held = format::number(node.oldest.into());
        meta(ui, &cx.fill("storage.held", &[&held, &format::mebibytes(node.stored_bytes)]), t);
        if let Some(fits) = node.fits {
            meta(ui, &cx.fill("storage.fits", &[&format::number(fits.into())]), t);
        }
        let mut keep_all = node.recent.is_none();
        if switch_row(ui, t, &mut keep_all, cx.say("storage.all")) {
            let recent = (!keep_all).then_some(self.recent.max(FEWEST_RECENT));
            cx.send(Command::Own(Personal::SetRecent(recent)));
        }
        if !keep_all {
            if self.recent == 0 {
                self.recent = node.recent.unwrap_or(FEWEST_RECENT);
            }
            labelled(ui, cx.say("storage.blocks"), |ui| {
                number_field(ui, &mut self.recent, 120.0);
                if button(ui, t, cx.say("mining.set"), Kind::Plain).clicked() {
                    cx.send(Command::Own(Personal::SetRecent(Some(self.recent))));
                }
            });
        }
        let mut merging = node.merging;
        if switch_row(ui, t, &mut merging, cx.say("storage.merge")) {
            cx.send(Command::Own(Personal::SetMerging(merging)));
        }
        if node.rebuilt {
            meta(ui, cx.say("storage.rebuilt"), t);
        }
    }
}

impl Page for SettingsPage {
    fn title(&self) -> &'static str {
        "page.settings"
    }

    fn icon(&self) -> Icon {
        Icon::Settings
    }

    fn apart(&self) -> bool {
        true
    }

    fn show(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        widgets::title(ui, cx.say("page.settings"));
        card(ui, t, |ui| Self::look(cx, ui));
        ui.add_space(tokens::GAP);
        card(ui, t, |ui| Self::mining(cx, ui));
        ui.add_space(tokens::GAP);
        card(ui, t, |ui| self.network(cx, ui));
        ui.add_space(tokens::GAP);
        card(ui, t, |ui| self.storage(cx, ui));
        ui.add_space(tokens::GAP);
        card(ui, t, |ui| {
            section(ui, cx.say("settings.setup"));
            let items: Vec<(Device, String)> = Device::ALL
                .iter()
                .map(|device| (*device, cx.say(device_name(*device)).to_string()))
                .collect();
            ui.horizontal_wrapped(|ui| {
                if let Some(device) = segments(ui, t, &items, Some(cx.settings.device)) {
                    cx.settings.apply_device(device);
                    cx.save();
                }
            });
            meta(ui, cx.say("settings.setup_note"), t);
            ui.add_space(4.0);
            meta(ui, cx.say("settings.folder"), t);
            ui.label(mono(cx.snap.folder.clone()));
        });
    }
}
