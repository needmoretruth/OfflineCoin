//! The wallet's quieter jobs, each one sheet: backup, restore, addresses, watching, and paying
//! from a computer that stays offline. Files are chosen by looking, and nothing asks for a path.
use super::{Cx, Slipping};
use crate::core::{parse_coins, Command, Personal};
use crate::ui::floating::{sheet, sheet_head};
use crate::ui::picker::{Picked, Picker};
use crate::ui::tokens;
use crate::ui::widgets::{
    button, button_on, field, meta, mono, secret_field, section, tag, Kind,
};
use crate::ui::format;
use egui::{Label, Ui};
use std::path::Path;

const BACKUP_ENDING: &str = "backup";
const KEY_ENDING: &str = "key";
const PAYMENT_ENDING: &str = "payment";
const SIGNED_ENDING: &str = "signed";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Backup,
    Restore,
    Addresses,
    Watch,
    Offline,
    Note,
}

/// What the file being chosen is for.
enum Filing {
    Backup,
    Restore,
    ExportKey(usize),
    Prepare { to: String, units: u64, fee: u64 },
    ToSign,
    Signed { from: String },
    Broadcast,
}

#[derive(Default)]
pub struct WalletTools {
    open: Option<Tool>,
    filing: Option<(Filing, Picker)>,
    password: String,
    import: String,
    watch: String,
    to: String,
    amount: String,
    fee: String,
}

fn own(cx: &Cx, order: Personal) {
    cx.send(Command::Own(order));
}

fn start<'a>(cx: &'a Cx<'_>) -> Option<&'a Path> {
    cx.folder.as_deref()
}

impl WalletTools {
    pub fn open(&mut self, tool: Tool) {
        self.open = Some(tool);
        self.password.clear();
    }

    fn password(&mut self) -> Option<String> {
        let password = std::mem::take(&mut self.password);
        (!password.is_empty()).then_some(password)
    }

    fn backup(&mut self, ui: &mut Ui, cx: &Cx) -> bool {
        let t = cx.t;
        sheet_head(ui, cx.say("backup.title"));
        meta(ui, cx.say("backup.body"), t);
        ui.add_space(4.0);
        secret_field(ui, &mut self.password, cx.say("backup.password"), ui.available_width());
        ui.add_space(tokens::GAP);
        let mut done = false;
        ui.horizontal(|ui| {
            if button(ui, t, cx.say("backup.choose"), Kind::Main).clicked() {
                let picker = Picker::save("backup.choose", "wallet", BACKUP_ENDING, start(cx));
                self.filing = Some((Filing::Backup, picker));
            }
            done = button(ui, t, cx.say("common.cancel"), Kind::Plain).clicked();
        });
        done
    }

    fn restore(&mut self, ui: &mut Ui, cx: &Cx) -> bool {
        let t = cx.t;
        sheet_head(ui, cx.say("restore.title"));
        meta(ui, cx.say("restore.body"), t);
        ui.add_space(4.0);
        secret_field(ui, &mut self.password, cx.say("restore.password"), ui.available_width());
        ui.add_space(tokens::GAP);
        let mut done = false;
        ui.horizontal(|ui| {
            if button(ui, t, cx.say("restore.choose"), Kind::Main).clicked() {
                let picker = Picker::open("restore.choose", BACKUP_ENDING, start(cx));
                self.filing = Some((Filing::Restore, picker));
            }
            done = button(ui, t, cx.say("common.cancel"), Kind::Plain).clicked();
        });
        done
    }

    fn addresses(&mut self, ui: &mut Ui, cx: &Cx) -> bool {
        let t = cx.t;
        sheet_head(ui, cx.say("keys.title"));
        for (index, key) in cx.snap.node.keys.iter().enumerate() {
            ui.add(Label::new(mono(key.address.clone())).wrap());
            ui.horizontal(|ui| {
                if key.receiving {
                    tag(ui, cx.say("keys.receiving"), t.good, t);
                } else if button(ui, t, cx.say("keys.use"), Kind::Plain).clicked() {
                    own(cx, Personal::UseKey(index));
                }
                if button(ui, t, cx.say("keys.copy"), Kind::Quiet).clicked() {
                    ui.ctx().copy_text(key.address.clone());
                }
                if button(ui, t, cx.say("keys.export"), Kind::Quiet).clicked() {
                    let picker = Picker::save("keys.export", "secret", KEY_ENDING, start(cx));
                    self.filing = Some((Filing::ExportKey(index), picker));
                }
            });
            if cx.expert() {
                meta(ui, &cx.fill("pro.public_key", &[&key.public]), t);
            }
            ui.add_space(tokens::GAP);
        }
        meta(ui, cx.say("keys.secret_note"), t);
        ui.add_space(tokens::GAP);
        ui.horizontal(|ui| {
            let width = ui.available_width() - 100.0;
            field(ui, &mut self.import, cx.say("keys.import_hint"), width);
            let ready = !self.import.trim().is_empty();
            if button_on(ui, t, cx.say("keys.import"), Kind::Plain, ready).clicked() {
                own(cx, Personal::ImportKey(std::mem::take(&mut self.import)));
            }
        });
        ui.add_space(tokens::GAP);
        let mut done = false;
        ui.horizontal(|ui| {
            if button(ui, t, cx.say("keys.new"), Kind::Plain).clicked() {
                own(cx, Personal::NewKey);
            }
            done = button(ui, t, cx.say("common.close"), Kind::Plain).clicked();
        });
        done
    }

    fn watched(&mut self, ui: &mut Ui, cx: &Cx) -> bool {
        let t = cx.t;
        sheet_head(ui, cx.say("watch.title"));
        meta(ui, cx.say("watch.body"), t);
        ui.add_space(4.0);
        let ticker = cx.ticker();
        for (address, coins) in &cx.snap.node.watched {
            ui.add(Label::new(mono(address.clone())).wrap());
            meta(ui, &format::coins(*coins, ticker), t);
        }
        ui.horizontal(|ui| {
            let width = ui.available_width() - 100.0;
            field(ui, &mut self.watch, cx.say("watch.hint"), width);
            let ready = !self.watch.trim().is_empty();
            if button_on(ui, t, cx.say("watch.add"), Kind::Plain, ready).clicked() {
                own(cx, Personal::Watch(std::mem::take(&mut self.watch)));
            }
        });
        ui.add_space(tokens::GAP);
        button(ui, t, cx.say("common.close"), Kind::Plain).clicked()
    }

    fn offline(&mut self, ui: &mut Ui, cx: &Cx) -> bool {
        let t = cx.t;
        sheet_head(ui, cx.say("offline.title"));
        meta(ui, cx.say("offline.body"), t);
        ui.add_space(tokens::GAP);
        section(ui, cx.say("offline.step_prepare"));
        meta(ui, cx.say("offline.prepare_about"), t);
        let width = ui.available_width();
        field(ui, &mut self.to, cx.say("send.to_hint"), width);
        ui.horizontal(|ui| {
            field(ui, &mut self.amount, cx.say("send.amount_hint"), 130.0);
            field(ui, &mut self.fee, cx.say("offline.fee_hint"), 130.0);
            let units = parse_coins(self.amount.trim());
            let fee = match self.fee.trim() {
                "" => cx.snap.node.coin.as_ref().map(|coin| coin.rules_at(cx.snap.node.height + 1).min_fee),
                text => parse_coins(text),
            };
            let ready = units.zip(fee).filter(|_| !self.to.trim().is_empty());
            if button_on(ui, t, cx.say("offline.prepare"), Kind::Plain, ready.is_some()).clicked() {
                if let Some((units, fee)) = ready {
                    let filing = Filing::Prepare { to: self.to.trim().to_string(), units, fee };
                    let picker = Picker::save("offline.prepare", "payment", PAYMENT_ENDING, start(cx));
                    self.filing = Some((filing, picker));
                }
            }
        });
        ui.add_space(tokens::GAP);
        section(ui, cx.say("offline.step_sign"));
        meta(ui, cx.say("offline.sign_about"), t);
        if button(ui, t, cx.say("offline.sign"), Kind::Plain).clicked() {
            let picker = Picker::open("offline.sign", PAYMENT_ENDING, start(cx));
            self.filing = Some((Filing::ToSign, picker));
        }
        ui.add_space(tokens::GAP);
        section(ui, cx.say("offline.step_broadcast"));
        meta(ui, cx.say("offline.broadcast_about"), t);
        if button(ui, t, cx.say("offline.broadcast"), Kind::Plain).clicked() {
            let picker = Picker::open("offline.broadcast", SIGNED_ENDING, start(cx));
            self.filing = Some((Filing::Broadcast, picker));
        }
        ui.add_space(tokens::GAP_WIDE);
        button(ui, t, cx.say("common.close"), Kind::Plain).clicked()
    }

    /// Acts on the file a person chose. True when the sheet is finished.
    fn chosen(&mut self, filing: Filing, path: String, cx: &mut Cx) -> bool {
        match filing {
            Filing::Backup => {
                let password = self.password();
                own(cx, Personal::BackUp { path, password });
                true
            }
            Filing::Restore => {
                let password = self.password();
                own(cx, Personal::Restore { path, password });
                true
            }
            Filing::ExportKey(index) => {
                own(cx, Personal::ExportKey { index, path });
                false
            }
            Filing::Prepare { to, units, fee } => {
                own(cx, Personal::Prepare { to, units, fee, save: Some(path) });
                self.amount.clear();
                false
            }
            Filing::ToSign => {
                let picker = Picker::save("offline.sign", "payment", SIGNED_ENDING, start(cx));
                self.filing = Some((Filing::Signed { from: path }, picker));
                false
            }
            Filing::Signed { from } => {
                own(cx, Personal::Sign { path: from, save: Some(path) });
                false
            }
            Filing::Broadcast => {
                *cx.slip = Some(cx.slip_for(Slipping {
                    what: cx.say("offline.broadcast"),
                    rows: vec![(cx.say("offline.file"), path.clone())],
                    warning: cx.say("send.warning"),
                    sign: cx.say("offline.broadcast"),
                    danger: false,
                    order: vec![Command::Own(Personal::Broadcast(path))],
                }));
                true
            }
        }
    }

    fn filing(&mut self, ui: &Ui, cx: &mut Cx) -> bool {
        let voice = cx.voice();
        let Some((_, picker)) = &mut self.filing else {
            return false;
        };
        match picker.show(ui, cx.t, voice) {
            Picked::Waiting => false,
            Picked::Cancelled => {
                self.filing = None;
                false
            }
            Picked::Chosen(path) => {
                *cx.folder = path.parent().map(Path::to_path_buf);
                match self.filing.take() {
                    Some((filing, _)) => self.chosen(filing, path.display().to_string(), cx),
                    None => false,
                }
            }
        }
    }

    pub fn show(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        let Some(tool) = self.open else {
            return;
        };
        let (done, outside) = sheet(ui, t, "wallet-tool", tokens::SHEET_WIDTH, |ui| match tool {
            Tool::Backup => self.backup(ui, cx),
            Tool::Restore => self.restore(ui, cx),
            Tool::Addresses => self.addresses(ui, cx),
            Tool::Watch => self.watched(ui, cx),
            Tool::Offline => self.offline(ui, cx),
            Tool::Note => super::note::show(ui, cx),
        });
        let finished = self.filing(ui, cx);
        if done || finished || (outside && self.filing.is_none()) {
            self.open = None;
            self.password.clear();
        }
    }
}
