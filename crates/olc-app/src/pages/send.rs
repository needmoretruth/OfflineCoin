//! The sheet that sends: one or more people in one payment, a name for it, and for an expert
//! the fee rate and the coins it spends. Nothing leaves before the slip is signed.
use super::money::{balance_of, can_pay, coin_of, named_field, parse_request, problem, shared_wallet, wallet_row};
use super::wallet_contacts as contacts;
use super::Cx;
use crate::core::{parse_coins, BoardAction, BoardView, Command, FeeLevels, LockOrder, Personal, WalletOrder};
use crate::ui::explain::{self, Choice};
use crate::ui::floating::{sheet_head, Slip};
use crate::ui::tokens;
use crate::ui::widgets::{self, button, button_on, meta, mono, segments, Kind};
use crate::ui::format;
use egui::{Align, Label, Layout, Ui};
use olc_control::BoardId;
use olc_wallet::choose::{estimated_size, fee_for};
use olc_wallet::parse_address;
use std::collections::BTreeSet;

/// People one payment pays at most here, which keeps the sheet on one screen.
const MOST_PEOPLE: usize = 8;
/// What the password prompt takes under the choices while the wallet is locked.
const UNLOCK_HEIGHT: f32 = 170.0;
/// The Next button, for a press that waits for the wallet to open.
const NEXT: u8 = 0;

/// How fast an expert wants the payment in a block.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Speed {
    /// The rate that gets into the next block as the pool stands: the automatic choice.
    #[default]
    Next,
    Within6,
    Lowest,
    Typed,
}

#[derive(Default)]
pub struct SendSheet {
    people: Vec<(String, String)>,
    label: String,
    tried: bool,
    payer: Option<BoardId>,
    speed: Speed,
    typed_rate: String,
    choosing: bool,
    chosen: BTreeSet<String>,
    unlock: super::wallet_seal::UnlockPrompt,
}

impl SendSheet {
    /// The board paying, when one was picked and it can still hear.
    fn board<'a>(&self, cx: &Cx<'a>) -> Option<&'a BoardView> {
        let chosen = |board: &&BoardView| Some(board.seen.id) == self.payer && can_pay(board);
        cx.snap.boards.iter().find(chosen)
    }

    /// One wallet needs no choosing, so a person with no boards sees no list.
    fn wallet_rows(&mut self, ui: &mut Ui, cx: &Cx) {
        if cx.snap.boards.is_empty() {
            return;
        }
        meta(ui, cx.say("send.from"), cx.t);
        let own = format::coins(cx.snap.node.spendable, cx.ticker());
        let on_board = self.board(cx).is_some();
        if wallet_row(ui, cx, cx.say("send.this_app"), &own, !on_board) {
            self.payer = None;
        }
        for board in &cx.snap.boards {
            let name = board.seen.beacon.name.as_str();
            if can_pay(board) {
                let current = self.board(cx).is_some_and(|paying| paying.seen.id == board.seen.id);
                if wallet_row(ui, cx, name, &balance_of(cx, board), current) {
                    self.payer = Some(board.seen.id);
                }
            } else {
                meta(ui, &cx.fill("send.off_board", &[name]), cx.t);
            }
        }
        ui.add_space(tokens::GAP);
    }

    /// Units a byte this payment offers.
    fn rate(&self, levels: &FeeLevels) -> u64 {
        match self.speed {
            Speed::Next => levels.rate_within(1),
            Speed::Within6 => levels.rate_within(6),
            Speed::Lowest => 0,
            Speed::Typed => self.typed_rate.trim().parse().unwrap_or(0),
        }
    }

    /// About how many bytes the payment takes: the coins it would spend, each person, and change.
    fn size(&self, cx: &Cx, total: u64) -> u64 {
        let coins = &cx.snap.node.wallet.coins;
        let inputs = match self.chosen.is_empty() {
            false => self.chosen.len(),
            true => {
                let (mut gathered, mut count) = (0u64, 0usize);
                for coin in coins.iter().filter(|coin| coin.ready) {
                    if gathered >= total {
                        break;
                    }
                    gathered = gathered.saturating_add(coin.value);
                    count += 1;
                }
                count
            }
        };
        estimated_size(inputs.max(1), self.people.len() + 1)
    }

    fn fee(&self, cx: &Cx, total: u64, min_fee: u64) -> u64 {
        let levels = &cx.snap.node.wallet.fees;
        fee_for(self.rate(levels), self.size(cx, total), min_fee)
    }

    /// The expert's fee levels, compared on the same three things, and a typed rate.
    fn speed_rows(&mut self, ui: &mut Ui, cx: &Cx, total: u64, min_fee: u64) {
        let t = cx.t;
        let levels = &cx.snap.node.wallet.fees;
        let size = self.size(cx, total);
        let choices = [(Speed::Next, "send.speed_next"), (Speed::Within6, "send.speed_six"), (Speed::Lowest, "send.speed_lowest"), (Speed::Typed, "send.speed_typed")];
        let items: Vec<(Speed, String)> = choices.iter().map(|(speed, key)| (*speed, cx.say(key).to_string())).collect();
        meta(ui, cx.say("send.speed"), t);
        if let Some(speed) = segments(ui, t, &items, Some(self.speed)) {
            self.speed = speed;
        }
        if self.speed == Speed::Typed {
            named_field(ui, t, cx.say("send.rate_hint"), &mut self.typed_rate, "", 160.0);
        }
        let fee = self.fee(cx, total, min_fee);
        let blocks = levels.blocks_for(fee, size);
        let wait = format::seconds(cx.voice(), blocks * levels.block_ms / 1_000);
        explain::short(ui, t, &cx.fill("send.speed_now", &[&format::coins(fee, cx.ticker()), &wait]));
        let mut values: Vec<[String; 3]> = Vec::new();
        for (speed, _) in &choices[..3] {
            let rate = Self { speed: *speed, ..Self::default() }.rate(levels);
            let fee = fee_for(rate, size, min_fee);
            let wait = format::seconds(cx.voice(), levels.blocks_for(fee, size) * levels.block_ms / 1_000);
            values.push([cx.fill("send.rate", &[&rate.to_string()]), format::coins(fee, cx.ticker()), wait]);
        }
        explain::details(ui, t, "send-speeds", (cx.say("explain.details"), cx.say("explain.hide")), |ui| {
            let shown: Vec<Vec<&str>> = values.iter().map(|row| row.iter().map(String::as_str).collect()).collect();
            let choices: Vec<Choice> = choices[..3].iter().zip(&shown).map(|((_, key), values)| Choice { name: cx.say(key), values }).collect();
            let criteria = [cx.say("send.c_rate"), cx.say("send.c_fee"), cx.say("send.c_wait")];
            explain::compare(ui, t, "send-speeds", &criteria, &choices);
            ui.add_space(tokens::GAP);
            meta(ui, &cx.fill("send.speeds_about", &[&levels.waiting().to_string()]), t);
        });
    }

    /// The expert's coin choice: none ticked means the wallet picks.
    fn coin_rows(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        ui.horizontal(|ui| {
            let key = if self.choosing { "send.coins_auto" } else { "send.coins_choose" };
            if button(ui, t, cx.say(key), Kind::Quiet).clicked() {
                self.choosing = !self.choosing;
                self.chosen.clear();
            }
            let picked: u64 = cx.snap.node.wallet.coins.iter().filter(|coin| self.chosen.contains(&coin.point)).map(|coin| coin.value).sum();
            if !self.chosen.is_empty() {
                meta(ui, &cx.fill("send.coins_picked", &[&self.chosen.len().to_string(), &format::coins(picked, cx.ticker())]), t);
            }
        });
        if !self.choosing {
            return;
        }
        explain::details(ui, t, "send-coins", (cx.say("explain.details"), cx.say("explain.hide")), |ui| {
            let auto = [cx.say("send.c_auto_picks"), cx.say("send.c_auto_for")];
            let hand = [cx.say("send.c_hand_picks"), cx.say("send.c_hand_for")];
            let choices = [Choice { name: cx.say("send.coins_auto"), values: &auto }, Choice { name: cx.say("send.coins_choose"), values: &hand }];
            explain::compare(ui, t, "send-coins", &[cx.say("send.c_picks"), cx.say("send.c_for")], &choices);
        });
        egui::ScrollArea::vertical().id_salt("send-coins").max_height(180.0).show(ui, |ui| {
            for coin in cx.snap.node.wallet.coins.iter().filter(|coin| coin.ready) {
                let mut on = self.chosen.contains(&coin.point);
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut on, format::coins(coin.value, cx.ticker())).changed() {
                        if on {
                            self.chosen.insert(coin.point.clone());
                        } else {
                            self.chosen.remove(&coin.point);
                        }
                    }
                    ui.add(Label::new(mono(format::short_address(&coin.address))).truncate());
                    meta(ui, &cx.fill("send.coin_block", &[&format::number(coin.height.into())]), t);
                });
            }
        });
    }

    fn slip(&self, cx: &Cx, payments: &[(String, u64)], fee: u64) -> Slip {
        let ticker = cx.ticker();
        let board = self.board(cx);
        let total: u64 = payments.iter().map(|(_, units)| units).sum();
        let have = board.map_or(cx.snap.node.spendable, |board| board.seen.beacon.balance);
        let mut rows = Vec::new();
        if let Some(board) = board {
            rows.push((cx.say("send.from").to_string(), board.seen.beacon.name.clone()));
        }
        let to = payments[0].0.clone();
        let shared = board.is_none().then(|| shared_wallet(&to)).flatten();
        for (address, units) in payments {
            let shown = match (&shared, contacts::name_of(cx, address)) {
                (Some(shared), _) => cx.fill("send.to_shared", &[&shared.need.to_string(), &shared.keys.len().to_string()]),
                (None, Some(name)) => format!("{name}  {}  {}", format::short_address(address), format::coins(*units, ticker)),
                (None, None) => format!("{}  {}", format::short_address(address), format::coins(*units, ticker)),
            };
            rows.push((cx.say("send.to").to_string(), shown));
        }
        if !self.label.trim().is_empty() {
            rows.push((cx.say("send.label").to_string(), self.label.trim().to_string()));
        }
        rows.push((cx.say("send.fee").to_string(), format::coins(fee, ticker)));
        rows.push((cx.say("send.left").to_string(), format::coins(have.saturating_sub(total + fee), ticker)));
        let amount = format::amount(total);
        let order = match board {
            Some(board) => Command::Board { id: board.seen.id, action: BoardAction::Pay { to, amount } },
            // A shared wallet's text stands for its lock, and coins go under it.
            None if shared.is_some() => Command::Lock(LockOrder::Pay { words: to, amount }),
            None => {
                let to = payments.iter().map(|(address, units)| (address.clone(), format::amount(*units))).collect();
                let coins = self.chosen.iter().cloned().collect();
                let order = WalletOrder::Pay { to, rate: self.rate(&cx.snap.node.wallet.fees), coins, label: self.label.trim().to_string() };
                Command::Own(Personal::Wallet(order))
            }
        };
        Slip {
            what: cx.say("send.slip").to_string(),
            amount: Some(format::coins(total, ticker)),
            rows,
            warning: Some(cx.say("send.warning").to_string()),
            sign: cx.say("send.sign").to_string(),
            cancel: cx.say("common.back").to_string(),
            danger: false,
            order: vec![order],
        }
    }

    /// One person as a unit: with several, a numbered card holding its address, its amount and,
    /// for an added person, its own Remove. Returns true when Remove was pressed.
    fn person(&mut self, ui: &mut Ui, cx: &Cx, at: usize, spendable: u64, fee: u64) -> bool {
        let gone = match self.people.len() > 1 {
            true => widgets::card_with_margin(ui, cx.t, 12, |ui| self.person_fields(ui, cx, at, spendable, fee)),
            false => self.person_fields(ui, cx, at, spendable, fee),
        };
        ui.add_space(tokens::GAP);
        gone
    }

    /// A person's two named fields. A request pasted in whole fills both.
    fn person_fields(&mut self, ui: &mut Ui, cx: &Cx, at: usize, spendable: u64, fee: u64) -> bool {
        let t = cx.t;
        let many = self.people.len() > 1;
        let mut gone = false;
        if many {
            ui.horizontal(|ui| {
                ui.label(widgets::medium_text(cx.fill("send.person", &[&(at + 1).to_string()])));
                if at > 0 {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        gone = button(ui, t, cx.say("send.remove_person"), Kind::Quiet).clicked();
                    });
                }
            });
        }
        let width = ui.available_width();
        let (to, amount) = &mut self.people[at];
        let hint = match cx.snap.node.wallet.contacts.is_empty() {
            true => "send.to_hint",
            false => "contacts.to_hint",
        };
        named_field(ui, t, cx.say(hint), to, "olc...", width);
        contacts::suggest(ui, cx, to);
        if let Some((address, units, note)) = parse_request(to) {
            *to = address;
            if let Some(units) = units {
                *amount = format::amount(units);
            }
            if !note.is_empty() && self.label.is_empty() {
                self.label = note;
            }
        }
        let (to, amount) = &mut self.people[at];
        // "All" fills one person's amount with everything that can go, so it shows only then.
        let room = if many { width } else { width - 110.0 };
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| named_field(ui, t, cx.say("send.amount_hint"), amount, "", room));
            if !many {
                ui.vertical(|ui| {
                    meta(ui, " ", t);
                    if button(ui, t, cx.say("send.all"), Kind::Plain).clicked() {
                        *amount = format::amount(spendable.saturating_sub(fee));
                    }
                });
            }
        });
        let shared = self.payer.is_none() && shared_wallet(to.trim()).is_some();
        if self.tried && parse_address(to.trim()).is_none() && !shared {
            problem(ui, t, cx.say("send.bad_address"));
        }
        if self.tried && parse_coins(amount.trim()).filter(|units| *units > 0).is_none() {
            problem(ui, t, cx.say("send.bad_amount"));
        }
        gone
    }

    /// True when it is finished.
    pub fn show(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let t = cx.t;
        if self.people.is_empty() {
            self.people.push(Default::default());
        }
        sheet_head(ui, cx.say("send.title"));
        self.wallet_rows(ui, cx);
        let node = &cx.snap.node;
        let board = self.board(cx);
        let coin = board.map_or(node.coin.as_ref(), |board| coin_of(cx, board));
        let next = board.map_or(node.height, |board| board.seen.beacon.height) + 1;
        let min_fee = coin.map_or(0, |coin| coin.rules_at(next).min_fee);
        let spendable = board.map_or(node.spendable, |board| board.seen.beacon.balance);
        meta(ui, &cx.fill("send.have", &[&format::coins(spendable, cx.ticker())]), t);
        if spendable == 0 {
            meta(ui, cx.say("send.none"), t);
        }
        ui.add_space(4.0);
        let units: Vec<Option<u64>> = self.people.iter().map(|(_, amount)| parse_coins(amount.trim()).filter(|units| *units > 0)).collect();
        let total: u64 = units.iter().flatten().sum();
        // A board or a shared wallet pays one person at the coin's lowest fee, as before.
        let own = board.is_none() && !self.people.iter().any(|(to, _)| shared_wallet(to.trim()).is_some());
        if !own {
            self.people.truncate(1);
        }
        let fee = if own { self.fee(cx, total, min_fee) } else { min_fee };
        let enough = total > 0 && total + fee <= spendable;
        // A board signs with its own key. This wallet signs with its own, so while it is locked
        // the password comes first, under the choices, before the slip.
        let waits = board.is_none() && super::wallet_seal::locked(cx);
        // People and choices scroll inside the sheet, so its title, the password and the buttons
        // stay on screen.
        let asking = if waits { UNLOCK_HEIGHT } else { 0.0 };
        let room = (ui.ctx().content_rect().height() - 260.0 - asking).max(160.0);
        let shared: &Cx = cx;
        egui::ScrollArea::vertical().id_salt("send-body").max_height(room).show(ui, |ui| {
            let mut gone = None;
            for at in 0..self.people.len() {
                if self.person(ui, shared, at, spendable, fee) {
                    gone = Some(at);
                }
            }
            if let Some(at) = gone {
                self.people.remove(at);
            }
            if own && self.people.len() < MOST_PEOPLE && button(ui, t, shared.say("send.add_person"), Kind::Quiet).clicked() {
                self.people.push(Default::default());
            }
            if own {
                named_field(ui, t, shared.say("send.label_hint"), &mut self.label, "", ui.available_width());
            }
            if own && shared.expert() {
                ui.add_space(tokens::GAP);
                self.speed_rows(ui, shared, total, min_fee);
                self.coin_rows(ui, shared);
            } else if own {
                meta(ui, &shared.fill("send.fee_auto", &[&format::coins(fee, shared.ticker())]), t);
            }
            if self.tried && total > 0 && !enough {
                problem(ui, t, shared.say("send.too_much"));
            }
        });
        ui.add_space(tokens::GAP);
        let mut entered = false;
        if waits {
            entered = self.unlock.ask(ui, cx);
            ui.add_space(tokens::GAP);
        }
        let mut done = false;
        ui.horizontal(|ui| {
            let filled = self.people.iter().all(|(to, amount)| !to.trim().is_empty() && !amount.trim().is_empty());
            // One press: Next opens a locked wallet with the password above and goes on to the slip.
            let ready = filled && (!waits || self.unlock.typed());
            let pressed = button_on(ui, t, cx.say("send.next"), Kind::Main, ready).clicked() || (ready && entered);
            let opened = self.unlock.opened(cx, NEXT);
            if pressed || opened {
                self.tried = true;
                let payments: Option<Vec<(String, u64)>> = self.people.iter().zip(&units).map(|((to, _), units)| {
                    let to = to.trim();
                    let fine = parse_address(to).is_some() || (self.payer.is_none() && shared_wallet(to).is_some());
                    fine.then_some(())?;
                    Some((to.to_string(), (*units)?))
                }).collect();
                // The password goes only with a payment that is ready, so a typo keeps it typed.
                let payments = payments.filter(|_| enough && (opened || self.unlock.press(cx, NEXT)));
                if let Some(payments) = payments {
                    if own {
                        let addresses: Vec<String> = payments.iter().map(|(address, _)| address.clone()).collect();
                        contacts::offer_after_paying(ui, cx, &addresses);
                    }
                    *cx.slip = Some(self.slip(cx, &payments, fee));
                    done = true;
                }
            }
            if button(ui, t, cx.say("common.cancel"), Kind::Plain).clicked() {
                done = true;
            }
        });
        done
    }
}
