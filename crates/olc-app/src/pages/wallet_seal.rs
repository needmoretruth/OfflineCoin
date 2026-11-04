//! The wallet's password on screen, as Bitcoin Core's Encrypt Wallet, Change Passphrase and its
//! unlock dialog: one sheet sets, opens, locks and changes it, and one short prompt sits in every
//! sheet whose work needs the keys while the wallet is locked. Balance, history and receiving
//! never ask.
use super::money::problem;
use super::Cx;
use crate::core::{Command, Password, PasswordOrder, Personal, WalletOrder, OPEN_FOR};
use crate::ui::explain;
use crate::ui::floating::sheet_head;
use crate::ui::tokens;
use crate::ui::widgets::{button, button_on, meta, secret_field, section, segments, Kind};
use egui::{Key, Response, Ui};

/// How long the screen offers to keep the wallet open, in seconds. Five minutes is the default.
const TIMES: [u64; 4] = [60, OPEN_FOR, 15 * 60, 60 * 60];

fn password_order(cx: &Cx, order: PasswordOrder) {
    cx.send(Command::Own(Personal::Wallet(WalletOrder::Password(order))));
}

/// True while the wallet has a password and is shut.
pub fn locked(cx: &Cx) -> bool {
    let lock = &cx.snap.node.wallet.lock;
    lock.sealed && lock.open_for.is_none()
}

/// A stretch of time as a person says it.
pub fn time_text(cx: &Cx, seconds: u64) -> String {
    if seconds < 60 {
        cx.fill("seal.seconds", &[&seconds.to_string()])
    } else if seconds < 60 * 60 {
        cx.fill("seal.minutes", &[&seconds.div_ceil(60).to_string()])
    } else {
        cx.fill("seal.hours", &[&seconds.div_ceil(60 * 60).to_string()])
    }
}

/// The line under the lock's row on the Wallet page: what the lock does, or how it stands.
pub fn row_about(cx: &Cx) -> String {
    let lock = &cx.snap.node.wallet.lock;
    match (lock.sealed, lock.open_for) {
        (false, _) => cx.say("seal.about_none").to_string(),
        (true, None) => cx.say("seal.about_locked").to_string(),
        (true, Some(left)) => cx.fill("seal.about_open", &[&time_text(cx, left)]),
    }
}

/// The button on that row: set a password, open the wallet, or look after an open one.
pub fn row_go(cx: &Cx) -> &'static str {
    let lock = &cx.snap.node.wallet.lock;
    match (lock.sealed, lock.open_for) {
        (false, _) => cx.say("seal.set"),
        (true, None) => cx.say("seal.open"),
        (true, Some(_)) => cx.say("seal.manage"),
    }
}

/// The row's name: what turning it on does, or, once on, the password it holds.
pub fn row_name(cx: &Cx) -> &'static str {
    match cx.snap.node.wallet.lock.sealed {
        false => cx.say("seal.row"),
        true => cx.say("seal.title"),
    }
}

/// A password field with its name above it, which stays once dots fill the field.
fn named_secret(ui: &mut Ui, cx: &Cx, text: &mut String, key: &str) -> Response {
    meta(ui, cx.say(key), cx.t);
    let field = secret_field(ui, text, cx.say(key), ui.available_width());
    ui.add_space(4.0);
    field
}

/// Enter in a field does what its sheet's main button does.
fn entered(ui: &Ui, field: &Response) -> bool {
    field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter))
}

/// Remembers how many wrong passwords the node had counted when one was sent, so the answer
/// shows beside the field that sent it.
#[derive(Default)]
struct Wrong(Option<u64>);

impl Wrong {
    fn sent(&mut self, cx: &Cx) {
        self.0 = Some(cx.snap.node.wallet.lock.wrong);
    }

    fn show(&mut self, ui: &mut Ui, cx: &Cx, typing: bool) {
        if typing {
            self.0 = None;
        }
        if self.0.is_some_and(|before| cx.snap.node.wallet.lock.wrong > before) {
            problem(ui, cx.t, cx.say("notice.wrong_wallet_password"));
        }
    }
}

/// The password once, and how long to keep the wallet open after it.
#[derive(Default)]
pub struct UnlockPrompt {
    password: String,
    seconds: Option<u64>,
    wrong: Wrong,
    /// The sheet's button that was pressed while the wallet was locked, waiting for it to open.
    waiting: Option<u8>,
}

impl UnlockPrompt {
    /// The password, the time to keep the wallet open and a button that opens it, for a sheet
    /// whose work is the opening itself.
    pub fn show(&mut self, ui: &mut Ui, cx: &Cx) {
        let field = self.ask(ui, cx);
        ui.add_space(4.0);
        let typed = self.typed();
        let pressed = button_on(ui, cx.t, cx.say("seal.open"), Kind::Main, typed).clicked();
        if typed && (pressed || field) {
            self.unlock(cx);
        }
    }

    /// The password and the time to keep the wallet open, with no button: the sheet's own button
    /// opens the wallet and goes on, through [`UnlockPrompt::press`]. True when Enter was pressed.
    pub fn ask(&mut self, ui: &mut Ui, cx: &Cx) -> bool {
        let t = cx.t;
        meta(ui, cx.say("seal.ask"), t);
        ui.add_space(4.0);
        let field = secret_field(ui, &mut self.password, cx.say("seal.password"), ui.available_width());
        self.wrong.show(ui, cx, field.changed());
        ui.add_space(4.0);
        meta(ui, cx.say("seal.keep"), t);
        let times: Vec<(u64, String)> = TIMES.iter().map(|seconds| (*seconds, time_text(cx, *seconds))).collect();
        if let Some(picked) = segments(ui, t, &times, Some(self.seconds.unwrap_or(OPEN_FOR))) {
            self.seconds = Some(picked);
        }
        entered(ui, &field)
    }

    pub fn typed(&self) -> bool {
        !self.password.is_empty()
    }

    fn unlock(&mut self, cx: &Cx) {
        self.wrong.sent(cx);
        let password = Password(std::mem::take(&mut self.password));
        password_order(cx, PasswordOrder::Unlock { password, seconds: self.seconds.unwrap_or(OPEN_FOR) });
    }

    /// A press of the sheet's button `which`. True when the wallet is open, so the work goes on
    /// now; while it is locked the typed password opens it, and [`UnlockPrompt::opened`] goes on.
    pub fn press(&mut self, cx: &Cx, which: u8) -> bool {
        if !locked(cx) {
            return true;
        }
        if self.typed() {
            self.waiting = Some(which);
            self.unlock(cx);
        }
        false
    }

    /// True once, on the frame the wallet opened for a press of `which` that waited for it. A
    /// wrong password ends the wait, and the line under the field says why.
    pub fn opened(&mut self, cx: &Cx, which: u8) -> bool {
        if self.waiting != Some(which) {
            return false;
        }
        if self.wrong.0.is_some_and(|before| cx.snap.node.wallet.lock.wrong > before) {
            self.waiting = None;
            return false;
        }
        if locked(cx) {
            return false;
        }
        self.waiting = None;
        true
    }
}

/// What a lost password leaves, behind Details.
fn lost(ui: &mut Ui, cx: &Cx) {
    let t = cx.t;
    explain::details(ui, t, "seal-lost", (cx.say("explain.details"), cx.say("explain.hide")), |ui| {
        ui.label(cx.say("seal.lost"));
        ui.add_space(4.0);
        ui.label(cx.say("seal.lost_backup"));
    });
}

/// The sheet behind the lock's row.
#[derive(Default)]
pub struct SealSheet {
    first: String,
    again: String,
    old: String,
    tried: bool,
    changing: bool,
    prompt: UnlockPrompt,
    wrong: Wrong,
}

impl SealSheet {
    /// A new password typed twice. True when both are there and the same.
    fn new_twice(&mut self, ui: &mut Ui, cx: &Cx) -> (bool, bool) {
        let first = named_secret(ui, cx, &mut self.first, "seal.new");
        let again = named_secret(ui, cx, &mut self.again, "seal.again");
        let differ = !self.again.is_empty() && self.first != self.again;
        if differ && (self.tried || again.lost_focus()) {
            problem(ui, cx.t, cx.say("seal.differ"));
        }
        let ready = !self.first.is_empty() && !differ && !self.again.is_empty();
        (ready, entered(ui, &first) || entered(ui, &again))
    }

    fn clear(&mut self) {
        self.first.clear();
        self.again.clear();
        self.old.clear();
        self.tried = false;
    }

    /// A wallet with no password yet: what a password changes, and the two fields.
    fn set(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        let view = &cx.snap.node.wallet;
        explain::short(ui, t, cx.say("seal.body"));
        if !view.has_words || view.words_to_write.is_some() {
            // A warning, not an error: yellow, as the app marks what still needs care.
            ui.add_space(4.0);
            ui.colored_label(t.warn, cx.say("seal.no_words"));
        }
        ui.add_space(tokens::GAP);
        let (ready, enter) = self.new_twice(ui, cx);
        ui.add_space(4.0);
        let pressed = button_on(ui, t, cx.say("seal.go"), Kind::Main, ready).clicked();
        if pressed || enter {
            self.tried = true;
        }
        if ready && (pressed || enter) {
            let password = Password(std::mem::take(&mut self.first));
            password_order(cx, PasswordOrder::Encrypt(password));
            self.clear();
        }
    }

    /// The old password and a new one twice.
    fn change(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        if !self.changing {
            if button(ui, t, cx.say("seal.change"), Kind::Plain).clicked() {
                self.changing = true;
            }
            return;
        }
        section(ui, cx.say("seal.change"));
        let old = named_secret(ui, cx, &mut self.old, "seal.old");
        self.wrong.show(ui, cx, old.changed());
        let (ready, enter) = self.new_twice(ui, cx);
        let ready = ready && !self.old.is_empty();
        ui.add_space(4.0);
        let mut pressed = false;
        ui.horizontal(|ui| {
            pressed = button_on(ui, t, cx.say("seal.change_go"), Kind::Main, ready).clicked();
            if button(ui, t, cx.say("common.cancel"), Kind::Plain).clicked() {
                self.changing = false;
                self.clear();
            }
        });
        if pressed || enter {
            self.tried = true;
        }
        if ready && (pressed || enter) {
            self.wrong.sent(cx);
            let old = Password(std::mem::take(&mut self.old));
            let new = Password(std::mem::take(&mut self.first));
            password_order(cx, PasswordOrder::Change { old, new });
            self.clear();
            self.changing = false;
        }
    }

    /// True when it is finished.
    pub fn show(&mut self, ui: &mut Ui, cx: &Cx) -> bool {
        let t = cx.t;
        let lock = cx.snap.node.wallet.lock.clone();
        sheet_head(ui, row_name(cx));
        match (lock.sealed, lock.open_for) {
            (false, _) => self.set(ui, cx),
            (true, None) => self.prompt.show(ui, cx),
            (true, Some(left)) => {
                meta(ui, &cx.fill("seal.about_open", &[&time_text(cx, left)]), t);
                ui.add_space(4.0);
                if button(ui, t, cx.say("seal.lock_now"), Kind::Main).clicked() {
                    password_order(cx, PasswordOrder::Lock);
                }
            }
        }
        ui.add_space(tokens::GAP);
        if lock.sealed {
            self.change(ui, cx);
            ui.add_space(tokens::GAP);
        }
        lost(ui, cx);
        ui.add_space(tokens::GAP);
        button(ui, t, cx.say("common.close"), Kind::Plain).clicked()
    }
}
