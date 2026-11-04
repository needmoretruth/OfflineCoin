//! Contacts: other people's addresses by name, as Bitcoin Core's address book. The Wallet page
//! lists them and adds, renames and deletes them. Send offers them by name as a person types, and
//! after paying an address nobody named it offers to save it.
use super::money::problem;
use super::Cx;
use crate::core::{Command, ContactOrder, Personal, WalletOrder, MOST_CONTACT_NAME};
use crate::ui::explain;
use crate::ui::floating::{sheet, sheet_head};
use crate::ui::format;
use crate::ui::tokens;
use crate::ui::widgets::{self, button, button_on, card, field, meta, mono, section, Kind};
use egui::text::{LayoutJob, TextFormat};
use egui::{vec2, Align, Id, Label, Layout, RichText, Ui};
use olc_wallet::parse_address;

/// Names Send offers at most while a person types.
const MOST_OFFERED: usize = 5;
/// How long an offer to save waits for its payment to go, in seconds.
const OFFER_WAIT: f64 = 120.0;

fn contact_order(cx: &Cx, order: ContactOrder) {
    cx.send(Command::Own(Personal::Wallet(WalletOrder::Contact(order))));
}

fn last_notice(cx: &Cx) -> u64 {
    cx.snap.notice.as_ref().map_or(0, |notice| notice.number)
}

/// The contact's name for an address, when it has one.
pub fn name_of<'a>(cx: &Cx<'a>, address: &str) -> Option<&'a str> {
    let contacts = &cx.snap.node.wallet.contacts;
    contacts.iter().find(|contact| contact.address == address.trim()).map(|contact| contact.name.as_str())
}

/// The section on the Wallet page.
#[derive(Default)]
pub struct ContactsCard {
    /// The name and the address of a contact being added.
    adding: Option<(String, String)>,
    /// The contact being changed: its name now, and the name typed.
    editing: Option<(String, String)>,
    tried: bool,
    /// The last notice when an order left, so its answer shows here.
    waiting: Option<u64>,
    /// The answer beside the form, and whether it is a refusal. An address saved already under
    /// another name is not one: it reads as a plain line.
    refused: Option<(String, bool)>,
}

impl ContactsCard {
    fn sent(&mut self, cx: &Cx, order: ContactOrder) {
        self.waiting = Some(last_notice(cx));
        self.refused = None;
        contact_order(cx, order);
    }

    /// Closes the form once its order went, or says why it did not.
    fn answer(&mut self, cx: &Cx) {
        let Some(sent) = self.waiting else {
            return;
        };
        let Some(notice) = cx.snap.notice.as_ref().filter(|notice| notice.number > sent) else {
            return;
        };
        self.waiting = None;
        match notice.key {
            "notice.contact_saved" | "notice.contact_removed" => {
                self.adding = None;
                self.editing = None;
                self.tried = false;
            }
            key if !notice.good || key == "notice.contact_known" => {
                self.refused = Some((cx.fill(key, &[&notice.detail]), !notice.good));
            }
            _ => {}
        }
    }

    fn refusal(&mut self, ui: &mut Ui, cx: &Cx) {
        match &self.refused {
            Some((why, true)) => problem(ui, cx.t, why),
            Some((line, false)) => {
                meta(ui, line, cx.t);
            }
            None => {}
        }
    }

    fn add_form(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        let width = ui.available_width();
        let Some((name, address)) = &mut self.adding else {
            return;
        };
        let typed = field(ui, name, cx.say("contacts.name"), width).changed() | field(ui, address, cx.say("contacts.address"), width).changed();
        let (name, address) = (name.trim().to_string(), address.trim().to_string());
        let named = !name.is_empty() && name.chars().count() <= MOST_CONTACT_NAME;
        let fine = parse_address(&address).is_some();
        if typed {
            self.refused = None;
        }
        if self.tried && !fine {
            problem(ui, t, cx.say("send.bad_address"));
        }
        self.refusal(ui, cx);
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if button_on(ui, t, cx.say("common.save"), Kind::Main, named && !address.is_empty()).clicked() {
                self.tried = true;
                if fine {
                    self.sent(cx, ContactOrder::Add { name, address });
                }
            }
            if button(ui, t, cx.say("common.cancel"), Kind::Plain).clicked() {
                self.adding = None;
                self.refused = None;
                self.tried = false;
            }
        });
    }

    fn edit_form(&mut self, ui: &mut Ui, cx: &Cx, address: &str) {
        let t = cx.t;
        let Some((name, typed)) = &mut self.editing else {
            return;
        };
        if field(ui, typed, cx.say("contacts.name"), ui.available_width()).changed() {
            self.refused = None;
        }
        ui.add(Label::new(mono(address).size(tokens::META).color(t.soft)).wrap());
        let (name, typed) = (name.clone(), typed.trim().to_string());
        self.refusal(ui, cx);
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let fine = !typed.is_empty() && typed.chars().count() <= MOST_CONTACT_NAME;
            if button_on(ui, t, cx.say("common.save"), Kind::Main, fine && typed != name).clicked() {
                self.sent(cx, ContactOrder::Rename { name: name.clone(), new_name: typed });
            }
            if button(ui, t, cx.say("contacts.remove"), Kind::Plain).clicked() {
                self.sent(cx, ContactOrder::Remove(name));
            }
            if button(ui, t, cx.say("common.cancel"), Kind::Quiet).clicked() {
                self.editing = None;
                self.refused = None;
            }
        });
    }

    /// One contact: its name and short address, and Edit on the right.
    fn row(&mut self, ui: &mut Ui, cx: &Cx, name: &str, address: &str) {
        let t = cx.t;
        if self.editing.as_ref().is_some_and(|(editing, _)| editing == name) {
            self.edit_form(ui, cx, address);
            ui.add_space(tokens::GAP);
            return;
        }
        let edit = cx.say("contacts.edit");
        let room = ui.available_width() - widgets::button_width(ui, edit) - tokens::GAP;
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(vec2(room, 0.0), Layout::top_down(Align::Min), |ui| {
                ui.set_width(room);
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.add(Label::new(RichText::new(name).color(t.text)).truncate());
                ui.add(Label::new(mono(format::short_address(address)).size(tokens::META).color(t.soft)));
            });
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                if button(ui, t, edit, Kind::Quiet).clicked() {
                    self.editing = Some((name.to_string(), name.to_string()));
                    self.adding = None;
                    self.refused = None;
                }
            });
        });
        ui.add_space(4.0);
    }

    pub fn show(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        self.answer(cx);
        let contacts = &cx.snap.node.wallet.contacts;
        card(ui, t, |ui| {
            section(ui, cx.say("contacts.title"));
            explain::short(ui, t, cx.say("contacts.about"));
            ui.add_space(tokens::GAP);
            if contacts.is_empty() && self.adding.is_none() {
                meta(ui, cx.say("contacts.empty"), t);
                ui.add_space(4.0);
            }
            // The page scrolls, so the list takes the room it needs: a box of its own inside the
            // page's scrolling showed a row and a half.
            for contact in contacts {
                self.row(ui, cx, &contact.name, &contact.address);
            }
            match self.adding.is_some() {
                true => self.add_form(ui, cx),
                false => {
                    if button(ui, t, cx.say("contacts.add"), Kind::Plain).clicked() {
                        self.adding = Some(Default::default());
                        self.editing = None;
                        self.refused = None;
                        self.tried = false;
                    }
                }
            }
        });
        ui.add_space(tokens::GAP);
    }
}

/// Under Send's address field: the contact an address belongs to, or contacts whose name holds
/// what is typed. Picking one puts its address in the field.
pub fn suggest(ui: &mut Ui, cx: &Cx, to: &mut String) {
    let t = cx.t;
    let typed = to.trim();
    if let Some(name) = name_of(cx, typed) {
        meta(ui, &cx.fill("contacts.to_named", &[name]), t);
        return;
    }
    if typed.is_empty() || parse_address(typed).is_some() {
        return;
    }
    let lower = typed.to_lowercase();
    let contacts = &cx.snap.node.wallet.contacts;
    let found: Vec<_> = contacts.iter().filter(|contact| contact.name.to_lowercase().contains(&lower)).take(MOST_OFFERED).collect();
    if found.is_empty() {
        return;
    }
    let width = ui.available_width();
    let body = egui::TextStyle::Body.resolve(ui.style());
    let code = egui::FontId::monospace(tokens::META);
    widgets::card_with_margin(ui, t, 6, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for contact in found {
            // The name as typed, its address softer beside it, so two people with one first
            // name still differ.
            let mut row = LayoutJob::default();
            row.append(&contact.name, 4.0, TextFormat::simple(body.clone(), t.text));
            row.append(&format::short_address(&contact.address), 16.0, TextFormat::simple(code.clone(), t.soft));
            let pick = egui::Button::new(row).frame(false).min_size(vec2(width - 12.0, tokens::CONTROL));
            let picked = ui.add(pick).on_hover_cursor(egui::CursorIcon::PointingHand);
            if picked.clicked() {
                *to = contact.address.clone();
            }
        }
    });
}

/// Addresses just paid that nobody named, offered for saving once the payment went.
#[derive(Clone, Default)]
struct Offer {
    addresses: Vec<(String, String)>,
    after: u64,
    since: f64,
    ready: bool,
}

fn offer_id() -> Id {
    Id::unique("contact-offer")
}

/// Send calls this as its slip opens. Addresses of this wallet and of contacts are left out.
pub fn offer_after_paying(ui: &Ui, cx: &Cx, addresses: &[String]) {
    let own = |address: &str| cx.snap.node.keys.iter().any(|key| key.address == address);
    let mut new: Vec<(String, String)> = Vec::new();
    for address in addresses {
        let known = name_of(cx, address).is_some() || own(address) || new.iter().any(|(seen, _)| seen == address);
        if !known && parse_address(address).is_some() {
            new.push((address.clone(), String::new()));
        }
    }
    let offer = Offer { addresses: new, after: last_notice(cx), since: ui.input(|input| input.time), ready: false };
    ui.ctx().data_mut(|data| match offer.addresses.is_empty() {
        true => data.remove::<Offer>(offer_id()),
        false => {
            data.insert_temp(offer_id(), offer);
        }
    });
}

/// The offer, once the payment it waits for went. A failed payment or a slip put back drops it.
pub fn offer(ui: &mut Ui, cx: &Cx) {
    let t = cx.t;
    let Some(mut offer) = ui.ctx().data(|data| data.get_temp::<Offer>(offer_id())) else {
        return;
    };
    let forget = |ui: &Ui| ui.ctx().data_mut(|data| data.remove::<Offer>(offer_id()));
    if !offer.ready {
        let answer = cx.snap.notice.as_ref().filter(|notice| notice.number > offer.after);
        match answer {
            Some(notice) if notice.key == "notice.sent" => offer.ready = true,
            Some(notice) if !notice.good => return forget(ui),
            _ if ui.input(|input| input.time) - offer.since > OFFER_WAIT => return forget(ui),
            _ => return,
        }
    }
    let mut finished = false;
    let (_, outside) = sheet(ui, t, "contact-offer", tokens::SLIP_WIDTH, |ui| {
        sheet_head(ui, cx.say("contacts.offer"));
        meta(ui, cx.say("contacts.offer_body"), t);
        ui.add_space(tokens::GAP);
        let width = ui.available_width();
        for (address, name) in &mut offer.addresses {
            ui.add(Label::new(mono(format::short_address(address)).color(t.soft)));
            field(ui, name, cx.say("contacts.name"), width);
            ui.add_space(4.0);
        }
        ui.add_space(tokens::GAP);
        ui.horizontal(|ui| {
            let named = offer.addresses.iter().any(|(_, name)| !name.trim().is_empty());
            if button_on(ui, t, cx.say("common.save"), Kind::Main, named).clicked() {
                for (address, name) in &offer.addresses {
                    if !name.trim().is_empty() {
                        contact_order(cx, ContactOrder::Add { name: name.trim().to_string(), address: address.clone() });
                    }
                }
                finished = true;
            }
            finished |= button(ui, t, cx.say("contacts.not_now"), Kind::Plain).clicked();
        });
    });
    match finished || outside {
        true => forget(ui),
        false => {
            ui.ctx().data_mut(|data| data.insert_temp(offer_id(), offer));
        }
    }
}
