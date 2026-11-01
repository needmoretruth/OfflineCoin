//! The pages. Adding one is one file and one line in [`all`].
mod boards;
mod experiments;
mod explorer;
mod keys;
mod local;
mod mining;
mod network;
mod pool;
mod settings;
pub mod setup;
mod wallet;

use crate::core::{Command, Core, Snapshot};
use crate::i18n::{self, Lang};
use crate::settings::Settings;
use crate::ui::{Icon, Tokens};
use egui::Ui;

/// What a page gets to read and change.
pub struct Cx<'a> {
    pub lang: Lang,
    pub snap: &'a Snapshot,
    pub settings: &'a mut Settings,
    pub t: &'a Tokens,
    pub core: &'a Core,
}

impl Cx<'_> {
    pub fn say(&self, key: &str) -> &'static str {
        i18n::text(self.lang, key)
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
    fn show(&mut self, ui: &mut Ui, cx: &mut Cx);
}

pub fn all() -> Vec<Box<dyn Page>> {
    vec![
        Box::<network::Network>::default(),
        Box::<mining::Mining>::default(),
        Box::<wallet::Wallet>::default(),
        Box::<keys::Keys>::default(),
        Box::<explorer::Explorer>::default(),
        Box::<boards::Boards>::default(),
        Box::<local::Local>::default(),
        Box::<pool::PoolPage>::default(),
        Box::<experiments::Experiments>::default(),
        Box::<settings::SettingsPage>::default(),
    ]
}
