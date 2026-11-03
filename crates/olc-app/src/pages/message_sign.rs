//! Signing a message with a key of this wallet, and checking one someone else signed, the way
//! Bitcoin's signmessage does it with OfflineCoin's own prefix.
use super::Cx;
use crate::core::{Command, Personal, WalletOrder};
use crate::ui::floating::sheet_head;
use crate::ui::tokens;
use crate::ui::widgets::{button, button_on, field, meta, mono, section, words_box, Kind};
use egui::{Label, Ui};

#[derive(Default)]
pub struct MessageSheet {
    address: String,
    message: String,
    signature: String,
    /// The notice whose signature was taken, so a later one is taken once too.
    taken: u64,
}

fn wallet(cx: &Cx, order: WalletOrder) {
    cx.send(Command::Own(Personal::Wallet(order)));
}

impl MessageSheet {
    /// True when it is finished.
    pub fn show(&mut self, ui: &mut Ui, cx: &Cx) -> bool {
        let t = cx.t;
        if let Some(notice) = cx.snap.notice.as_ref().filter(|notice| notice.key == "notice.message_signed") {
            if notice.number != self.taken {
                self.taken = notice.number;
                self.signature = notice.detail.clone();
            }
        }
        sheet_head(ui, cx.say("sign.title"));
        meta(ui, cx.say("sign.body"), t);
        ui.add_space(tokens::GAP);
        let width = ui.available_width();
        if self.address.is_empty() {
            self.address = cx.snap.node.address.clone();
        }
        field(ui, &mut self.address, cx.say("sign.address"), width);
        words_box(ui, &mut self.message, cx.say("sign.text"), width);
        field(ui, &mut self.signature, cx.say("sign.signature"), width);
        ui.add_space(tokens::GAP);
        let mut done = false;
        ui.horizontal(|ui| {
            let written = !self.message.is_empty() && !self.address.trim().is_empty();
            if button_on(ui, t, cx.say("sign.sign"), Kind::Main, written).clicked() {
                let (address, message) = (self.address.trim().to_string(), self.message.clone());
                wallet(cx, WalletOrder::SignMessage { address, message });
            }
            let signed = written && !self.signature.trim().is_empty();
            if button_on(ui, t, cx.say("sign.verify"), Kind::Plain, signed).clicked() {
                let address = self.address.trim().to_string();
                let (signature, message) = (self.signature.trim().to_string(), self.message.clone());
                wallet(cx, WalletOrder::VerifyMessage { address, signature, message });
            }
            done = button(ui, t, cx.say("common.close"), Kind::Plain).clicked();
        });
        if !self.signature.is_empty() {
            ui.add_space(4.0);
            section(ui, cx.say("sign.signature"));
            ui.add(Label::new(mono(self.signature.clone())).wrap());
            if button(ui, t, cx.say("shared.copy"), Kind::Quiet).clicked() {
                ui.ctx().copy_text(self.signature.clone());
            }
        }
        done
    }
}
