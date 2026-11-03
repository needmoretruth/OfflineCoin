//! The places of the app. Adding a place is one file and one line in [`all`]. A page takes the
//! mode and shows or hides rows, so nothing is drawn twice.
mod board_detail;
mod board_more;
mod boards;
mod channels;
mod coin_fork;
mod coin_new;
mod coin_rows;
mod coins;
mod console;
mod home;
mod lab;
mod local;
mod locks;
mod message_sign;
mod money;
mod note;
mod pool;
mod records;
pub mod reach;
mod settings;
mod settings_more;
pub(crate) mod seed_words;
mod shared_wallets;
pub mod setup;
mod wallet_tools;

use crate::core::{Command, Core, Snapshot};
use crate::i18n::{self, Lang, Mode, Voice};
use crate::settings::Settings;
use crate::ui::floating::Slip;
use crate::ui::{Icon, Tokens};
use egui::Ui;
use std::path::PathBuf;

/// A slip with its words still as keys of the table.
pub struct Slipping {
    pub what: &'static str,
    pub rows: Vec<(&'static str, String)>,
    pub warning: &'static str,
    pub sign: &'static str,
    pub danger: bool,
    pub order: Vec<Command>,
}

/// What a page gets to read and change.
pub struct Cx<'a> {
    pub lang: Lang,
    pub snap: &'a Snapshot,
    pub settings: &'a mut Settings,
    pub t: &'a Tokens,
    pub core: &'a Core,
    /// A slip to read before anything happens. The window shows it over the page.
    pub slip: &'a mut Option<Slip>,
    /// A place to go to next.
    pub go: &'a mut Option<&'static str>,
    /// Where the last file was chosen, so the next one starts there.
    pub folder: &'a mut Option<PathBuf>,
}

impl Cx<'_> {
    pub fn voice(&self) -> Voice {
        Voice { lang: self.lang, mode: self.settings.mode }
    }

    pub fn slip_for(&self, slip: Slipping) -> Slip {
        Slip {
            what: slip.what.to_string(),
            amount: None,
            rows: slip.rows.into_iter().map(|(name, value)| (name.to_string(), value)).collect(),
            warning: Some(slip.warning.to_string()).filter(|text| !text.is_empty()),
            sign: slip.sign.to_string(),
            cancel: self.say("common.back").to_string(),
            danger: slip.danger,
            order: slip.order,
        }
    }

    pub fn expert(&self) -> bool {
        self.settings.mode == Mode::Expert
    }

    pub fn ticker(&self) -> &str {
        self.snap.node.coin.as_ref().map_or("", |coin| coin.ticker.as_str())
    }

    pub fn say(&self, key: &str) -> &'static str {
        i18n::text(self.voice(), key)
    }

    pub fn fill(&self, key: &str, values: &[&str]) -> String {
        i18n::fill(self.say(key), values)
    }

    pub fn send(&self, command: Command) {
        self.core.send(command);
    }

    pub fn save(&self) {
        self.send(Command::Save(Box::new(self.settings.clone())));
    }
}

pub trait Page {
    /// The key of its name.
    fn title(&self) -> &'static str;
    fn icon(&self) -> Icon;
    /// Kept apart at the bottom of the side bar.
    fn apart(&self) -> bool {
        false
    }
    /// Only an expert sees it.
    fn expert_only(&self) -> bool {
        false
    }
    fn show(&mut self, ui: &mut Ui, cx: &mut Cx);
}

pub fn all() -> Vec<Box<dyn Page>> {
    vec![
        Box::<home::Home>::default(),
        Box::<boards::Boards>::default(),
        Box::<coins::Coins>::default(),
        Box::<records::Records>::default(),
        Box::<channels::Channels>::default(),
        Box::<locks::Locks>::default(),
        Box::<console::Console>::default(),
        Box::<settings::SettingsPage>::default(),
    ]
}
