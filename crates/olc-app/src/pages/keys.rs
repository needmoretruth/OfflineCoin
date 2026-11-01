//! The keys this node holds, and the wallet file. A secret key never shows on screen.
use super::{Cx, Page};
use crate::core::{Command, Personal};
use crate::ui::tokens;
use crate::ui::widgets::{
    self, button, button_on, card, chip, field, meta, mono, secret_field, section, Kind,
};
use crate::ui::Icon;
use egui::Ui;

#[derive(Default)]
pub struct Keys {
    import: String,
    export_path: String,
    backup_path: String,
    password: String,
}

fn own(cx: &Cx, order: Personal) {
    cx.send(Command::Own(order));
}

impl Keys {
    fn list(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        section(ui, cx.say("keys.title"));
        for (index, key) in cx.snap.node.keys.iter().enumerate() {
            ui.horizontal_wrapped(|ui| {
                ui.label(mono(key.address.clone()));
                if button(ui, t, cx.say("wallet.copy"), Kind::Quiet).clicked() {
                    ui.ctx().copy_text(key.address.clone());
                }
                if key.receiving {
                    chip(ui, cx.say("keys.receiving"), t.good, t);
                } else if button(ui, t, cx.say("keys.use"), Kind::Plain).clicked() {
                    own(cx, Personal::UseKey(index));
                }
                let ready = !self.export_path.trim().is_empty();
                if button_on(ui, t, cx.say("keys.export"), Kind::Plain, ready).clicked() {
                    let path = std::mem::take(&mut self.export_path);
                    own(cx, Personal::ExportKey { index, path });
                }
            });
            meta(ui, &cx.fill("keys.public", &[&key.public]), t);
            ui.add_space(4.0);
        }
        field(ui, &mut self.export_path, cx.say("keys.export_hint"), 380.0);
        meta(ui, cx.say("keys.secret_note"), t);
    }

    fn add(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        if button(ui, t, cx.say("keys.new"), Kind::Main).clicked() {
            own(cx, Personal::NewKey);
        }
        ui.horizontal(|ui| {
            secret_field(ui, &mut self.import, cx.say("keys.import_hint"), 380.0);
            let ready = !self.import.trim().is_empty();
            if button_on(ui, t, cx.say("keys.import"), Kind::Plain, ready).clicked() {
                own(cx, Personal::ImportKey(std::mem::take(&mut self.import)));
            }
        });
    }

    fn backup(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        section(ui, cx.say("keys.backup_title"));
        field(ui, &mut self.backup_path, cx.say("keys.backup_path"), 380.0);
        secret_field(ui, &mut self.password, cx.say("keys.password"), 380.0);
        let password = (!self.password.is_empty()).then(|| self.password.clone());
        let path = self.backup_path.trim().to_string();
        ui.horizontal(|ui| {
            let ready = !path.is_empty();
            if button_on(ui, t, cx.say("keys.backup"), Kind::Main, ready).clicked() {
                let (path, password) = (path.clone(), password.clone());
                own(cx, Personal::BackUp { path, password });
                self.password.clear();
            }
            if button_on(ui, t, cx.say("keys.restore"), Kind::Plain, ready).clicked() {
                own(cx, Personal::Restore { path: path.clone(), password: password.clone() });
                self.password.clear();
            }
        });
        meta(ui, cx.say("keys.backup_note"), t);
    }
}

impl Page for Keys {
    fn title(&self) -> &'static str {
        "page.keys"
    }

    fn icon(&self) -> Icon {
        Icon::Keys
    }

    fn show(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        widgets::title(ui, cx.say("page.keys"));
        card(ui, t, |ui| self.list(ui, cx));
        ui.add_space(tokens::GAP);
        card(ui, t, |ui| self.add(ui, cx));
        ui.add_space(tokens::GAP);
        card(ui, t, |ui| self.backup(ui, cx));
    }
}
