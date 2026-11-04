//! The wallet's quieter jobs, each one sheet: backup, restore, addresses, watching, and paying
//! from a computer that stays offline. Files are chosen by looking, and nothing asks for a path.
use super::wallet_seal::{locked, UnlockPrompt};
use super::{Cx, Slipping};
use crate::core::{parse_coins, Command, Personal, WalletOrder};
use crate::ui::explain;
use crate::ui::floating::{sheet, sheet_head};
use crate::ui::picker::{Picked, Picker};
use crate::ui::tokens;
use crate::ui::widgets::{
    self, button, button_on, field, meta, mono, secret_field, section, tag, Kind,
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
    Words,
    Shared,
    Message,
    Seal,
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
    /// The address being named, and the name typed so far.
    naming: Option<(String, String)>,
    to: String,
    amount: String,
    fee: String,
    words: super::seed_words::WordsSheet,
    shared: super::shared_wallets::SharedSheet,
    message: super::message_sign::MessageSheet,
    seal: super::wallet_seal::SealSheet,
    /// The password, for a sheet whose work needs the keys of a locked wallet.
    unlock: UnlockPrompt,
    /// A locked wallet's backup with a password of its own, or none, which needs it open.
    other_password: bool,
}

/// Over a sheet whose work needs the keys: the password, while the wallet is locked. True when it
/// showed, and the work waits.
fn asks_password(ui: &mut Ui, cx: &Cx, unlock: &mut UnlockPrompt) -> bool {
    if !locked(cx) {
        return false;
    }
    unlock.show(ui, cx);
    ui.add_space(tokens::GAP);
    true
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
        self.other_password = false;
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
        // A locked wallet's backup keeps its keys sealed as they are. Another password, or none,
        // needs the keys, and so the wallet open first.
        let sealed_copy = locked(cx) && !self.other_password;
        if sealed_copy {
            explain::short(ui, t, cx.say("seal.backup_locked"));
            if button(ui, t, cx.say("seal.backup_open"), Kind::Quiet).clicked() {
                self.other_password = true;
            }
        } else if !asks_password(ui, cx, &mut self.unlock) {
            secret_field(ui, &mut self.password, cx.say("backup.password"), ui.available_width());
        }
        ui.add_space(tokens::GAP);
        let mut done = false;
        let ready = sealed_copy || !locked(cx);
        ui.horizontal(|ui| {
            if button_on(ui, t, cx.say("backup.choose"), Kind::Main, ready).clicked() {
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
        // Restored keys are sealed with the rest, which needs the wallet open.
        let waits = asks_password(ui, cx, &mut self.unlock);
        if !waits {
            secret_field(ui, &mut self.password, cx.say("restore.password"), ui.available_width());
        }
        ui.add_space(tokens::GAP);
        let mut done = false;
        ui.horizontal(|ui| {
            if button_on(ui, t, cx.say("restore.choose"), Kind::Main, !waits).clicked() {
                let picker = Picker::open("restore.choose", BACKUP_ENDING, start(cx));
                self.filing = Some((Filing::Restore, picker));
            }
            done = button(ui, t, cx.say("common.cancel"), Kind::Plain).clicked();
        });
        done
    }

    /// An address's name, which opens into a field to change it.
    fn name_line(&mut self, ui: &mut Ui, cx: &Cx, address: &str) {
        let t = cx.t;
        let name = cx.snap.node.wallet.labels.get(address);
        let editing = self.naming.as_ref().is_some_and(|(named, _)| named == address);
        if !editing {
            ui.horizontal(|ui| {
                match name {
                    Some(name) => ui.label(name.as_str()),
                    None => ui.label(egui::RichText::new(cx.say("keys.no_name")).color(t.soft)),
                };
                if button(ui, t, cx.say("keys.name"), Kind::Quiet).clicked() {
                    self.naming = Some((address.to_string(), name.cloned().unwrap_or_default()));
                }
            });
            return;
        }
        let mut finished = false;
        if let Some((_, typed)) = &mut self.naming {
            meta(ui, cx.say("keys.name_hint"), t);
            ui.horizontal(|ui| {
                let width = (ui.available_width() - 100.0).max(120.0);
                field(ui, typed, "", width);
                if button(ui, t, cx.say("keys.name_save"), Kind::Plain).clicked() {
                    let order = WalletOrder::Label { of: address.to_string(), name: typed.trim().to_string() };
                    cx.send(Command::Own(Personal::Wallet(order)));
                    finished = true;
                }
            });
        }
        if finished {
            self.naming = None;
        }
    }

    fn addresses(&mut self, ui: &mut Ui, cx: &Cx) -> bool {
        let t = cx.t;
        sheet_head(ui, cx.say("keys.title"));
        let ticker = cx.ticker();
        for (index, key) in cx.snap.node.keys.iter().enumerate() {
            let held = cx.snap.node.wallet.addresses.iter().find(|held| held.address == key.address);
            ui.horizontal(|ui| {
                ui.add(Label::new(mono(key.address.clone())).truncate());
                if let Some(held) = held {
                    let coins = cx.fill("keys.balance", &[&format::coins(held.balance, ticker), &held.coins.to_string()]);
                    ui.label(widgets::medium_text(coins));
                }
            });
            self.name_line(ui, cx, &key.address);
            ui.horizontal(|ui| {
                if key.receiving {
                    tag(ui, cx.say("keys.receiving"), t.good, t);
                } else if button(ui, t, cx.say("keys.use"), Kind::Plain).clicked() {
                    own(cx, Personal::UseKey(index));
                }
                if button(ui, t, cx.say("keys.copy"), Kind::Quiet).clicked() {
                    ui.ctx().copy_text(key.address.clone());
                }
                if button_on(ui, t, cx.say("keys.export"), Kind::Quiet, !locked(cx)).clicked() {
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
        // A new key, one brought in and one taken out all need the wallet open.
        let open = !asks_password(ui, cx, &mut self.unlock);
        ui.horizontal(|ui| {
            let width = ui.available_width() - 100.0;
            field(ui, &mut self.import, cx.say("keys.import_hint"), width);
            let ready = !self.import.trim().is_empty() && open;
            if button_on(ui, t, cx.say("keys.import"), Kind::Plain, ready).clicked() {
                own(cx, Personal::ImportKey(std::mem::take(&mut self.import)));
            }
        });
        ui.add_space(tokens::GAP);
        let mut done = false;
        ui.horizontal(|ui| {
            if button_on(ui, t, cx.say("keys.new"), Kind::Plain, open).clicked() {
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
            ui.horizontal(|ui| {
                ui.add(Label::new(mono(address.clone())).truncate());
                ui.label(widgets::medium_text(format::coins(*coins, ticker)));
                if button(ui, t, cx.say("watch.remove"), Kind::Quiet).clicked() {
                    cx.send(Command::Own(Personal::Wallet(WalletOrder::Unwatch(address.clone()))));
                }
            });
            self.name_line(ui, cx, address);
            ui.add_space(4.0);
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
        let open = !asks_password(ui, cx, &mut self.unlock);
        if button_on(ui, t, cx.say("offline.sign"), Kind::Plain, open).clicked() {
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
            Tool::Words => self.words.show(ui, cx),
            Tool::Shared => self.shared.show(ui, cx),
            Tool::Message => self.message.show(ui, cx),
            Tool::Seal => self.seal.show(ui, cx),
        });
        let finished = self.filing(ui, cx);
        if done || finished || (outside && self.filing.is_none()) {
            self.open = None;
            self.password.clear();
        }
    }
}
