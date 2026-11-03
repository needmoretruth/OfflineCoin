//! The register: every choice with its value and what it means, in three columns.
use super::settings_more::{Extras, ExtraValues};
use super::wallet_tools::{Tool, WalletTools};
use super::{Cx, Page};
use crate::core::Command;
use crate::i18n::{Lang, Mode};
use crate::settings::{Device, Theme, RELAXED_CAP};
use crate::ui::format;
use crate::ui::register::{group, row};
use crate::ui::widgets::{self, button, meta, number_field, segments, switch, Kind};
use crate::ui::{Icon, Tokens};
use egui::Ui;

const TEXT_SIZES: [u16; 3] = [100, 125, 150];
/// Tall enough for all five devices at the largest text size, with no scroll bar.
const DEVICE_LIST_HEIGHT: f32 = 420.0;

pub fn device_name(device: Device) -> &'static str {
    match device {
        Device::Esp32 => "setup.esp32",
        Device::RaspberryPi => "setup.pi",
        Device::ThisComputer => "setup.this_computer",
        Device::SeveralNodes => "setup.several_nodes",
        Device::TwoComputers => "setup.two_computers",
    }
}

#[derive(Default)]
pub struct SettingsPage {
    tools: WalletTools,
    extras: ExtraValues,
}

/// A row with a switch for one setting.
pub fn switch_setting(ui: &mut Ui, cx: &mut Cx, keys: (&str, &str), read: fn(&mut crate::settings::Settings) -> &mut bool) {
    let t = cx.t;
    let name = cx.say(keys.0);
    row(ui, t, name, cx.say(keys.1), |ui| {
        if switch(ui, t, read(cx.settings), name).clicked() {
            cx.save();
        }
    });
}

fn opening_row(ui: &mut Ui, t: &Tokens, cx: &Cx, keys: (&str, &str, &str)) -> bool {
    let mut open = false;
    row(ui, t, cx.say(keys.0), cx.say(keys.1), |ui| {
        open = button(ui, t, cx.say(keys.2), Kind::Plain).clicked();
    });
    open
}

fn general(ui: &mut Ui, cx: &mut Cx) {
    let t = cx.t;
    group(ui, cx.say("settings.general"));
    row(ui, t, cx.say("settings.language"), cx.say("settings.language_about"), |ui| {
        let items = [(Lang::En, "English".to_string()), (Lang::Ko, "한국어".to_string())];
        if let Some(language) = segments(ui, t, &items, Some(cx.settings.language)) {
            cx.settings.language = language;
            cx.save();
        }
    });
    row(ui, t, cx.say("settings.theme"), cx.say("settings.theme_about"), |ui| {
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
    row(ui, t, cx.say("settings.text"), cx.say("settings.text_about"), |ui| {
        let items = TEXT_SIZES.map(|percent| (percent, format!("{percent}%")));
        if let Some(percent) = segments(ui, t, &items, Some(cx.settings.text_percent)) {
            cx.settings.text_percent = percent;
            cx.save();
        }
    });
    row(ui, t, cx.say("settings.mode"), cx.say("settings.mode_about"), |ui| {
        let items = [
            (Mode::Basic, cx.say("settings.mode_basic").to_string()),
            (Mode::Expert, cx.say("settings.mode_expert").to_string()),
        ];
        if let Some(mode) = segments(ui, t, &items, Some(cx.settings.mode)) {
            cx.settings.mode = mode;
            cx.save();
        }
    });
    let keys = ("settings.reduce_motion", "settings.reduce_motion_about");
    switch_setting(ui, cx, keys, |settings| &mut settings.reduce_motion);
}

impl SettingsPage {
    fn about(&mut self, ui: &mut Ui, cx: &Cx) {
        group(ui, cx.say("settings.about"));
        if opening_row(ui, cx.t, cx, ("settings.note", "settings.note_about", "settings.note_go")) {
            self.tools.open(Tool::Note);
        }
    }

    fn wallet(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        group(ui, cx.say("settings.wallet"));
        let rows = [
            (("settings.backup", "settings.backup_about", "settings.backup_go"), Tool::Backup),
            (("settings.restore", "settings.restore_about", "settings.restore_go"), Tool::Restore),
            (("settings.addresses", "settings.addresses_about", "common.open"), Tool::Addresses),
            (("settings.watch", "settings.watch_about", "common.open"), Tool::Watch),
            (("settings.offline", "settings.offline_about", "common.open"), Tool::Offline),
            (("settings.words", "settings.words_about", "common.open"), Tool::Words),
            (("settings.shared", "settings.shared_about", "common.open"), Tool::Shared),
        ];
        for (keys, tool) in rows {
            if opening_row(ui, t, cx, keys) {
                self.tools.open(tool);
            }
        }
        if cx.expert() && opening_row(ui, t, cx, ("pro.sign_message", "pro.sign_message_about", "common.open")) {
            self.tools.open(Tool::Message);
        }
    }
}

fn speed_row(ui: &mut Ui, cx: &mut Cx) {
    let t = cx.t;
    let node = &cx.snap.node;
    let voice = cx.voice();
    let full = cx.snap.full_rate;
    let about = cx.fill("settings.speed_about", &[&format::rate(voice, full)]);
    row(ui, t, cx.say("settings.speed"), &about, |ui| {
        let items = [
            (RELAXED_CAP, cx.say("settings.speed_gentle").to_string()),
            (full / 2, cx.say("settings.speed_half").to_string()),
            (0, cx.say("settings.speed_full").to_string()),
        ];
        if let Some(cap) = segments(ui, t, &items, Some(node.cap)) {
            cx.settings.hash_cap = cap;
            cx.send(Command::SetCap(cap));
            cx.save();
        }
    });
    if cx.expert() {
        row(ui, t, cx.say("pro.cap"), cx.say("pro.cap_about"), |ui| {
            let name = cx.say("pro.cap");
            if number_field(ui, &mut cx.settings.hash_cap, name, 120.0) {
                cx.save();
            }
            if button(ui, t, cx.say("common.apply"), Kind::Plain).clicked() {
                cx.send(Command::SetCap(cx.settings.hash_cap));
            }
        });
        row(ui, t, cx.say("pro.board_cap"), cx.say("pro.board_cap_about"), |ui| {
            let name = cx.say("pro.board_cap");
            if number_field(ui, &mut cx.settings.board_cap, name, 120.0) {
                cx.settings.board_cap_chosen = true;
                cx.save();
            }
        });
    }
}

fn this_computer(ui: &mut Ui, cx: &mut Cx) {
    let t = cx.t;
    group(ui, cx.say("settings.computer"));
    let about = cx.say("settings.device_about");
    row(ui, t, cx.say("settings.device"), about, |ui| {
        // Five names do not fit this column as a row of segments at any text size.
        let mut chosen = cx.settings.device;
        egui::ComboBox::from_id_salt("device")
            .selected_text(cx.say(device_name(chosen)))
            .height(DEVICE_LIST_HEIGHT)
            .show_ui(ui, |ui| {
                for device in Device::ALL {
                    ui.selectable_value(&mut chosen, device, cx.say(device_name(device)));
                }
            });
        if chosen != cx.settings.device {
            cx.settings.apply_device(chosen);
            cx.save();
        }
    });
    row(ui, t, cx.say("settings.data"), &cx.snap.folder.clone(), |ui| {
        if button(ui, t, cx.say("settings.open_data"), Kind::Plain).clicked() {
            cx.send(Command::OpenDataFolder);
        }
    });
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
        ui.spacing_mut().item_spacing.y = 0.0;
        if cx.expert() {
            meta(ui, cx.say("pro.settings_note"), t);
        }
        general(ui, cx);
        self.wallet(ui, cx);
        group(ui, cx.say("settings.mining"));
        speed_row(ui, cx);
        Extras { values: &mut self.extras }.show(ui, cx);
        this_computer(ui, cx);
        self.about(ui, cx);
        self.tools.show(ui, cx);
    }
}
