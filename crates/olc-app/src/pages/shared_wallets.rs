//! Wallets several people share: made from the public keys the others give, paid like any
//! address, and spent once enough of them have signed. A spend goes from one signer to the next
//! as text or a code.
use super::money::code;
use super::Cx;
use crate::core::{Command, LockOrder, Personal, SharedView, WalletOrder};
use crate::ui::format;
use crate::ui::floating::sheet_head;
use crate::ui::tokens;
use crate::ui::widgets::{button, button_on, card, field, meta, mono, section, words_box, Kind};
use egui::{Label, RichText, ScrollArea, Ui};
use olc_service::qr::qr_grid;

const MOST_KEYS: u64 = 20;

#[derive(Default)]
pub struct SharedSheet {
    need: u64,
    keys: String,
    added: String,
    /// The wallet whose code or payment form is open.
    open: Option<(usize, Doing)>,
    to: String,
    amount: String,
    passed: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Doing {
    Receive,
    Pay,
}

fn wallet(cx: &Cx, order: WalletOrder) {
    cx.send(Command::Own(Personal::Wallet(order)));
}

/// Text to pass on, its code when it fits in one, and a button that copies it.
fn passing(ui: &mut Ui, cx: &Cx, text: &str) {
    if let Some(grid) = qr_grid(text) {
        ui.vertical_centered(|ui| code(ui, &grid, cx.say("shared.code")));
        ui.add_space(4.0);
    }
    ui.add(Label::new(mono(text.to_string())).wrap());
    if button(ui, cx.t, cx.say("shared.copy"), Kind::Plain).clicked() {
        ui.ctx().copy_text(text.to_string());
    }
}

impl SharedSheet {
    fn my_key(&self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        section(ui, cx.say("shared.my_key"));
        meta(ui, cx.say("shared.my_key_body"), t);
        let mine = cx.snap.node.keys.iter().find(|key| key.receiving);
        if let Some(key) = mine {
            ui.add(Label::new(mono(key.public.clone())).wrap());
            if button(ui, t, cx.say("shared.copy"), Kind::Plain).clicked() {
                ui.ctx().copy_text(key.public.clone());
            }
        }
    }

    fn one(&mut self, ui: &mut Ui, cx: &Cx, index: usize, shared: &SharedView) {
        let t = cx.t;
        card(ui, t, |ui| {
            let title = cx.fill("shared.who", &[&shared.need.to_string(), &shared.signers.len().to_string()]);
            ui.horizontal(|ui| {
                ui.add(Label::new(RichText::new(title).strong()).selectable(false));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format::coins(shared.balance, cx.ticker()));
                });
            });
            let mine: Vec<String> = shared.mine.iter().map(|at| (at + 1).to_string()).collect();
            meta(ui, &cx.fill("shared.mine", &[&mine.join(", ")]), t);
            ui.horizontal(|ui| {
                if button(ui, t, cx.say("shared.receive"), Kind::Plain).clicked() {
                    self.open = Some((index, Doing::Receive));
                }
                if button_on(ui, t, cx.say("shared.pay"), Kind::Plain, shared.balance > 0).clicked() {
                    self.open = Some((index, Doing::Pay));
                }
            });
            match self.open {
                Some((at, Doing::Receive)) if at == index => {
                    meta(ui, cx.say("shared.receive_body"), t);
                    passing(ui, cx, &shared.text);
                }
                Some((at, Doing::Pay)) if at == index => self.pay(ui, cx, index),
                _ => {}
            }
        });
    }

    fn pay(&mut self, ui: &mut Ui, cx: &Cx, index: usize) {
        let t = cx.t;
        let width = ui.available_width();
        field(ui, &mut self.to, cx.say("send.to_hint"), width);
        field(ui, &mut self.amount, cx.say("send.amount_hint"), width);
        let filled = !self.to.trim().is_empty() && !self.amount.trim().is_empty();
        if button_on(ui, t, cx.say("shared.sign_pay"), Kind::Main, filled).clicked() {
            let (to, amount) = (self.to.trim().to_string(), self.amount.trim().to_string());
            cx.send(Command::Lock(LockOrder::PayShared { wallet: index, to, amount }));
            self.open = None;
        }
    }

    /// The spend that waits for others, and a place for one another signer passed here.
    fn signatures(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        section(ui, cx.say("shared.signing"));
        let waiting = cx.snap.node.half_spend.as_ref().filter(|(text, _)| text.starts_with("olcsharedspend1"));
        if let Some((text, who)) = waiting {
            // Signers by their place in the wallet, as "my place: 1" names this one.
            let signers = cx.snap.node.wallet.shared.iter().find(|shared| who.iter().all(|address| shared.signers.contains(address)));
            let places: Vec<String> = who
                .iter()
                .filter_map(|address| signers?.signers.iter().position(|signer| signer == address))
                .map(|place| cx.fill("shared.place", &[&(place + 1).to_string()]))
                .collect();
            meta(ui, &cx.fill("shared.waiting", &[&places.join(", ")]), t);
            if cx.expert() {
                meta(ui, &who.join(", "), t);
            }
            passing(ui, cx, text);
            ui.add_space(tokens::GAP);
        }
        meta(ui, cx.say("shared.passed_body"), t);
        words_box(ui, &mut self.passed, cx.say("shared.passed_hint"), ui.available_width());
        if button_on(ui, t, cx.say("shared.sign"), Kind::Plain, !self.passed.trim().is_empty()).clicked() {
            let text = std::mem::take(&mut self.passed).trim().to_string();
            cx.send(Command::Lock(LockOrder::Sign { text }));
        }
    }

    fn make(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        section(ui, cx.say("shared.make"));
        meta(ui, cx.say("shared.make_body"), t);
        if self.keys.is_empty() {
            if let Some(key) = cx.snap.node.keys.iter().find(|key| key.receiving) {
                self.keys = format!("{}\n", key.public);
            }
        }
        words_box(ui, &mut self.keys, cx.say("shared.keys_hint"), ui.available_width());
        let count = self.keys.split_whitespace().count() as u64;
        if self.need == 0 {
            self.need = 2;
        }
        ui.horizontal(|ui| {
            ui.label(cx.say("shared.need"));
            crate::ui::widgets::number_field(ui, &mut self.need, cx.say("shared.need"), 60.0);
            ui.label(cx.fill("shared.of", &[&count.to_string()]));
        });
        let fits = (1..=count).contains(&self.need) && (2..=MOST_KEYS).contains(&count);
        if !fits && count > 0 {
            ui.colored_label(t.bad, cx.say("shared.bad_count"));
        }
        if button_on(ui, t, cx.say("shared.make_go"), Kind::Main, fits).clicked() {
            let keys = self.keys.split_whitespace().map(str::to_string).collect();
            wallet(cx, WalletOrder::MakeShared { need: self.need as usize, keys });
            self.keys.clear();
        }
        ui.add_space(tokens::GAP);
        meta(ui, cx.say("shared.add_body"), t);
        field(ui, &mut self.added, cx.say("shared.add_hint"), ui.available_width());
        if button_on(ui, t, cx.say("shared.add"), Kind::Plain, !self.added.trim().is_empty()).clicked() {
            wallet(cx, WalletOrder::AddShared(std::mem::take(&mut self.added).trim().to_string()));
        }
    }

    /// True when it is finished.
    pub fn show(&mut self, ui: &mut Ui, cx: &Cx) -> bool {
        sheet_head(ui, cx.say("shared.title"));
        meta(ui, cx.say("shared.body"), cx.t);
        ui.add_space(tokens::GAP);
        let mut done = false;
        // The title, the line under it and the close button keep their room; the rest scrolls.
        let room = (ui.ctx().content_rect().height() - 280.0).max(300.0);
        ScrollArea::vertical().max_height(room).show(ui, |ui| {
            // A spend that waits for others is the thing to do next, so it comes first.
            let waiting = cx.snap.node.half_spend.as_ref().is_some_and(|(text, _)| text.starts_with("olcsharedspend1"));
            if waiting {
                self.signatures(ui, cx);
                ui.add_space(tokens::GAP);
            }
            let wallets = cx.snap.node.wallet.shared.clone();
            for (index, shared) in wallets.iter().enumerate() {
                self.one(ui, cx, index, shared);
                ui.add_space(8.0);
            }
            if !waiting {
                self.signatures(ui, cx);
            }
            ui.add_space(tokens::GAP);
            self.make(ui, cx);
            ui.add_space(tokens::GAP);
            self.my_key(ui, cx);
        });
        ui.add_space(tokens::GAP);
        done |= button(ui, cx.t, cx.say("common.close"), Kind::Plain).clicked();
        done
    }
}
