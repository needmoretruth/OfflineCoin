//! Two choices in Settings put side by side on the same criteria, behind Details: the two modes,
//! and how much of the chain to keep. A value names the trade it makes, so neither option is
//! favoured by the wording.
use super::Cx;
use crate::i18n::{self, Mode, Voice};
use crate::ui::explain::{self, Choice};
use crate::ui::format;
use egui::Ui;
use olc_core::FEWEST_RECENT;

/// Basic and expert mode on what words they use, what they show and where things are. The
/// example words are the two modes' own names for one setting, read from their tables.
pub fn modes(ui: &mut Ui, cx: &Cx) {
    let words_in = |mode| i18n::text(Voice { lang: cx.lang, mode }, "storage.merge");
    let basic = [
        cx.fill("settings.v_words_basic", &[words_in(Mode::Basic)]),
        cx.say("settings.v_shows_basic").to_string(),
        cx.say("settings.v_places_basic").to_string(),
    ];
    let expert = [
        cx.fill("settings.v_words_expert", &[words_in(Mode::Expert)]),
        cx.say("settings.v_shows_expert").to_string(),
        cx.say("settings.v_places_expert").to_string(),
    ];
    let criteria = [cx.say("settings.c_words"), cx.say("settings.c_shows"), cx.say("settings.c_places")];
    let values = [basic, expert];
    let names = [cx.say("settings.mode_basic"), cx.say("settings.mode_expert")];
    compared(ui, cx, "modes", &criteria, &names, &values);
}

/// Everything, recent blocks or the summaries only, on whether it mines, what it keeps, what it
/// gives a new computer and who checks the coins. `recent` is the number of blocks kept now.
pub fn keeping(ui: &mut Ui, cx: &Cx, recent: Option<u64>) {
    let recent = format::number(recent.unwrap_or(FEWEST_RECENT).into());
    let say = |key: &str| cx.say(key).to_string();
    let values = [
        [say("settings.v_yes"), say("settings.v_disk_all"), say("settings.v_gives_all"), say("settings.v_checks_all")],
        [
            say("settings.v_yes"),
            cx.fill("settings.v_disk_recent", &[&recent]),
            say("settings.v_gives_recent"),
            say("settings.v_checks_recent"),
        ],
        [say("settings.v_no"), say("settings.v_disk_headers"), say("settings.v_gives_none"), say("settings.v_checks_headers")],
    ];
    let criteria = [cx.say("settings.c_mines"), cx.say("settings.c_disk"), cx.say("settings.c_gives"), cx.say("settings.c_checks")];
    let names = [cx.say("local.keep_all"), cx.say("local.keep_recent"), cx.say("local.keep_headers")];
    compared(ui, cx, "keeping", &criteria, &names, &values);
}

fn compared<const N: usize>(ui: &mut Ui, cx: &Cx, id: &str, criteria: &[&str], names: &[&str], values: &[[String; N]]) {
    let shown: Vec<Vec<&str>> = values.iter().map(|row| row.iter().map(String::as_str).collect()).collect();
    let choices: Vec<Choice> = names.iter().zip(&shown).map(|(name, values)| Choice { name, values }).collect();
    explain::compare(ui, cx.t, id, criteria, &choices);
}
