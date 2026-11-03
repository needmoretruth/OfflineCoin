//! The register: every choice with its value and what it means, in three columns.
use super::settings_more::{Extras, ExtraValues};
use super::wallet_tools::{Tool, WalletTools};
use super::{Cx, Page};
use crate::core::Command;
use crate::i18n::{Lang, Mode};
use crate::settings::{cores, standard_threads, Device, Speed, Theme};
use crate::ui::format;
use crate::ui::register::{group, row};
use crate::ui::widgets::{self, button, field, meta, segments, switch, Kind};
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
            cx.settings.language_chosen = true;
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

fn speed_name(speed: Speed) -> &'static str {
    match speed {
        Speed::Quarter => "settings.speed_quarter",
        Speed::Half => "settings.speed_half",
        Speed::Full => "settings.speed_full",
    }
}

/// A number taken once, when Enter is pressed or the field is left, so a cap of 50,000 is not
/// first 5, 50, 500 and 5,000 on the way. `shown` is the value in force.
pub fn committed_number(ui: &mut Ui, shown: u64, name: &str, width: f32) -> Option<u64> {
    let id = ui.next_auto_id().with("typed");
    let mut typed: String = ui.data_mut(|data| data.get_temp(id)).unwrap_or_else(|| shown.to_string());
    let response = field(ui, &mut typed, name, width);
    if response.has_focus() {
        ui.data_mut(|data| data.insert_temp(id, typed.clone()));
    } else {
        ui.data_mut(|data| data.remove_temp::<String>(id));
    }
    let number = typed.trim().parse::<u64>().ok();
    number.filter(|number| response.lost_focus() && *number != shown)
}

/// The node takes the cap at once, and the saved copy holds it too.
fn set_cap(cx: &mut Cx, cap: u64) {
    cx.send(Command::SetCap(cap));
    cx.settings.hash_cap = cap;
    cx.save();
}

/// Speed in steps that are parts of this computer's full rate; in expert mode also the exact
/// cap and the threads. The cap shown is the one the node runs under.
fn speed_row(ui: &mut Ui, cx: &mut Cx) {
    let t = cx.t;
    let cap = cx.snap.node.cap;
    let voice = cx.voice();
    let full = cx.snap.full_rate;
    let threads = cx.settings.threads(standard_threads());
    let about = cx.fill("settings.speed_about", &[&format::rate(voice, full), &threads.to_string()]);
    row(ui, t, cx.say("settings.speed"), &about, |ui| {
        let items = Speed::ALL.map(|speed| (speed, cx.say(speed_name(speed)).to_string()));
        if let Some(speed) = segments(ui, t, &items, Speed::of(cap, full)) {
            set_cap(cx, speed.cap(full));
        }
    });
    if cx.expert() {
        row(ui, t, cx.say("pro.cap"), cx.say("pro.cap_about"), |ui| {
            if let Some(typed) = committed_number(ui, cap, cx.say("pro.cap"), 120.0) {
                set_cap(cx, typed);
            }
        });
        let about = cx.fill("pro.threads_about", &[&cores().to_string()]);
        row(ui, t, cx.say("pro.threads"), &about, |ui| {
            if let Some(typed) = committed_number(ui, threads as u64, cx.say("pro.threads"), 60.0) {
                cx.settings.mining_threads = typed.clamp(1, cores() as u64) as u32;
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

#[cfg(test)]
mod tests {
    use super::committed_number;
    use egui::accesskit::Role;
    use egui::{Key, Modifiers};
    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;

    /// Typing 50000 into the cap field takes nothing on the way, and 50,000 once on Enter.
    #[test]
    fn a_typed_cap_applies_once_on_enter() {
        // Fonts land on the next frame, so the first one only sets them.
        let take = |ui: &mut egui::Ui, (fonts, taken): &mut (bool, Vec<u64>)| {
            if !*fonts {
                *fonts = true;
                ui.ctx().set_fonts(crate::ui::fonts::definitions());
                return;
            }
            if let Some(number) = committed_number(ui, 20_000, "Cap", 120.0) {
                taken.push(number);
            }
        };
        let mut harness = Harness::new_ui_state(take, (false, Vec::new()));
        harness.run();
        harness.get_by_role(Role::TextInput).focus();
        harness.run();
        harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
        harness.run();
        for digit in ["5", "0", "0", "0", "0"] {
            harness.get_by_role(Role::TextInput).type_text(digit);
            harness.run();
        }
        assert!(harness.state().1.is_empty(), "taken while typing: {:?}", harness.state().1);
        harness.key_press(Key::Enter);
        harness.run();
        assert_eq!(harness.state().1, vec![50_000]);
    }
}
