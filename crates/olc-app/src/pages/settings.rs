//! Settings: preferences only, in the four groups of Bitcoin Core's options window (General,
//! Mining, Network, Display), and under every name whether it takes hold now or from the next
//! start. The wallet's tools live on the Wallet page and the speed on Mining; at the bottom the
//! settings file opens and everything here goes back to how it came.
use super::settings_compare;
use super::settings_more::{Extras, ExtraValues};
use super::wallet_tools::{Tool, WalletTools};
use super::{Cx, Page};
use crate::core::Command;
use crate::i18n::{Lang, Mode};
use crate::settings::{cores, standard_threads, Settings, Theme};
use crate::ui::format;
use crate::ui::register::{group, row, setting, setting_details, When};
use crate::ui::tokens;
use crate::ui::widgets::{self, button, field, meta, segments, switch, Kind};
use crate::ui::Icon;
use egui::Ui;

const TEXT_SIZES: [u16; 3] = [100, 125, 150];

#[derive(Default)]
pub struct SettingsPage {
    tools: WalletTools,
    extras: ExtraValues,
    /// Reset was pressed once; the second press resets.
    resetting: bool,
}

/// The words under a setting's name that say when it takes hold.
pub fn when(cx: &Cx, when: When) -> (When, &'static str) {
    match when {
        When::Now => (when, cx.say("settings.now")),
        When::NextStart => (when, cx.say("settings.next_start")),
    }
}

/// A row with a switch for one setting.
pub fn switch_setting(ui: &mut Ui, cx: &mut Cx, keys: (&str, &str), taken: When, read: fn(&mut Settings) -> &mut bool) {
    let t = cx.t;
    let name = cx.say(keys.0);
    setting(ui, t, name, when(cx, taken), cx.say(keys.1), |ui| {
        if switch(ui, t, read(cx.settings), name).clicked() {
            cx.save();
        }
    });
}

/// Language, look, text size and motion: all of them change the window at once.
fn display(ui: &mut Ui, cx: &mut Cx) {
    let t = cx.t;
    let now = when(cx, When::Now);
    group(ui, cx.say("settings.display"));
    setting(ui, t, cx.say("settings.language"), now, cx.say("settings.language_about"), |ui| {
        let items = [(Lang::En, "English".to_string()), (Lang::Ko, "한국어".to_string())];
        if let Some(language) = segments(ui, t, &items, Some(cx.settings.language)) {
            cx.settings.language = language;
            cx.settings.language_chosen = true;
            cx.save();
        }
    });
    setting(ui, t, cx.say("settings.theme"), now, cx.say("settings.theme_about"), |ui| {
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
    setting(ui, t, cx.say("settings.text"), now, cx.say("settings.text_about"), |ui| {
        let items = TEXT_SIZES.map(|percent| (percent, format!("{percent}%")));
        if let Some(percent) = segments(ui, t, &items, Some(cx.settings.text_percent)) {
            cx.settings.text_percent = percent;
            cx.save();
        }
    });
    let keys = ("settings.reduce_motion", "settings.reduce_motion_about");
    switch_setting(ui, cx, keys, When::Now, |settings| &mut settings.reduce_motion);
}

/// The mode, with the two compared behind Details.
fn mode_row(ui: &mut Ui, cx: &mut Cx) {
    let t = cx.t;
    let items = [
        (Mode::Basic, cx.say("settings.mode_basic").to_string()),
        (Mode::Expert, cx.say("settings.mode_expert").to_string()),
    ];
    let mut chosen = None;
    let name = (cx.say("settings.mode"), when(cx, When::Now));
    let labels = (cx.say("explain.details"), cx.say("explain.hide"));
    let control = |ui: &mut Ui| chosen = segments(ui, t, &items, Some(cx.settings.mode));
    setting_details(ui, t, name, (cx.say("settings.mode_about"), "modes", labels), control, |ui| {
        settings_compare::modes(ui, cx);
    });
    if let Some(mode) = chosen {
        cx.settings.mode = mode;
        cx.save();
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

/// The speed is set where the mining is: this row says it and goes there. In expert mode the
/// threads follow, since they set what full speed is.
pub fn speed_rows(ui: &mut Ui, cx: &mut Cx) {
    let t = cx.t;
    let voice = cx.voice();
    let (cap, full) = (cx.snap.node.cap, cx.snap.full_rate);
    // A cap of 0, or one above what this computer can do, holds nothing back.
    let now = match cap == 0 || (full > 0 && cap >= full) {
        true => cx.fill("rate.max", &[&format::rate(voice, full)]),
        false => cx.fill("rate.of", &[&format::rate(voice, cap), &format::rate(voice, full)]),
    };
    setting(ui, t, cx.say("settings.speed"), when(cx, When::Now), &now, |ui| {
        if button(ui, t, cx.say("settings.speed_go"), Kind::Plain).clicked() {
            *cx.go = Some("page.mining");
        }
    });
    if !cx.expert() {
        return;
    }
    let threads = cx.settings.threads(standard_threads());
    let about = cx.fill("pro.threads_about", &[&cores().to_string()]);
    setting(ui, t, cx.say("pro.threads"), when(cx, When::Now), &about, |ui| {
        if let Some(typed) = committed_number(ui, threads as u64, cx.say("pro.threads"), 60.0) {
            cx.settings.mining_threads = typed.clamp(1, cores() as u64) as u32;
            cx.save();
        }
    });
}

/// What this screen sets, as it came, with the rest of the file kept: the language a person
/// picked, the coin, the miners, the boards and the peers are not preferences.
fn reset(settings: &Settings) -> Settings {
    let fresh = Settings::default();
    Settings {
        mode: fresh.mode,
        theme: fresh.theme,
        reduce_motion: fresh.reduce_motion,
        text_percent: fresh.text_percent,
        hash_cap: fresh.hash_cap,
        mining_threads: fresh.mining_threads,
        mine_on_start: fresh.mine_on_start,
        port: fresh.port,
        listen_lan: fresh.listen_lan,
        scan_lan: fresh.scan_lan,
        broadcast: fresh.broadcast,
        scan_ports: fresh.scan_ports,
        auto_update: fresh.auto_update,
        ..settings.clone()
    }
}

impl SettingsPage {
    /// The data folder and the note sit with the general rows.
    fn general(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        group(ui, cx.say("settings.general"));
        mode_row(ui, cx);
        Extras { values: &mut self.extras }.keeping(ui, cx);
        let keys = ("settings.auto_update", "settings.auto_update_about");
        switch_setting(ui, cx, keys, When::Now, |settings| &mut settings.auto_update);
        row(ui, t, cx.say("settings.data"), &cx.snap.folder.clone(), |ui| {
            if button(ui, t, cx.say("settings.open_data"), Kind::Plain).clicked() {
                cx.send(Command::OpenDataFolder);
            }
        });
        row(ui, t, cx.say("settings.note"), cx.say("settings.note_about"), |ui| {
            if button(ui, t, cx.say("settings.note_go"), Kind::Plain).clicked() {
                self.tools.open(Tool::Note);
            }
        });
    }

    /// Bitcoin Core's two buttons under its options: the file, and everything back as it came.
    /// Going back takes a second press, since it undoes every row above at once.
    fn file(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        ui.add_space(tokens::GAP_WIDE);
        ui.horizontal(|ui| {
            if button(ui, t, cx.say("settings.file"), Kind::Plain).clicked() {
                cx.send(Command::OpenSettingsFile);
            }
            let (label, kind) = match self.resetting {
                false => (cx.say("settings.reset"), Kind::Plain),
                true => (cx.say("settings.reset_sure"), Kind::Danger),
            };
            if button(ui, t, label, kind).clicked() {
                if self.resetting {
                    *cx.settings = reset(cx.settings);
                    cx.save();
                }
                self.resetting = !self.resetting;
            }
            if self.resetting && button(ui, t, cx.say("common.cancel"), Kind::Quiet).clicked() {
                self.resetting = false;
            }
        });
        ui.add_space(tokens::GAP);
        let about = if self.resetting { "settings.reset_about" } else { "settings.file_about" };
        meta(ui, cx.say(about), t);
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
        widgets::title(ui, cx.say("page.settings"));
        ui.spacing_mut().item_spacing.y = 0.0;
        self.general(ui, cx);
        Extras { values: &mut self.extras }.mining(ui, cx);
        Extras { values: &mut self.extras }.network(ui, cx);
        display(ui, cx);
        self.file(ui, cx);
        self.tools.show(ui, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::{committed_number, reset};
    use crate::i18n::{Lang, Mode};
    use crate::settings::Settings;
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

    /// Going back to defaults takes the preferences back and keeps the language, the coin and
    /// the peers a person had.
    #[test]
    fn reset_keeps_what_is_not_a_preference() {
        let mine = Settings {
            mode: Mode::Expert,
            hash_cap: 3_000,
            listen_lan: false,
            language: Lang::Ko,
            language_chosen: true,
            coin: Some("Race".into()),
            peers: vec!["10.0.0.2:7690".into()],
            setup_done: true,
            ..Settings::default()
        };
        let back = reset(&mine);
        let fresh = Settings::default();
        assert_eq!((back.mode, back.hash_cap, back.listen_lan), (fresh.mode, fresh.hash_cap, fresh.listen_lan));
        assert_eq!((back.language, back.language_chosen, back.setup_done), (Lang::Ko, true, true));
        assert_eq!((back.coin, back.peers), (mine.coin, mine.peers));
    }
}
