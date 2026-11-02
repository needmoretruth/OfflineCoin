//! One board as a register: its name, what it does and how it connects.
use super::board_more::expert_rows;
use super::coins::reach_text;
use super::{Cx, Slipping};
use crate::core::{BoardAction, BoardView, Command};
use crate::ui::format;
use crate::ui::register::{group, row};
use crate::ui::tokens;
use crate::ui::widgets::{
    button, button_on, field, meta, number_field, secret_field, segments, slot, switch, Kind,
};
use egui::Ui;
use olc_control::defaults::{BOARD_BRIGHT, BOARD_SCREEN_RATE};
use olc_control::BoardId;
use std::collections::HashMap;

/// Dim, middle and bright, on the board's own scale of 0 to 255.
const BRIGHT_STEPS: [u64; 3] = [24, 96, 255];
const BRIGHT_MAX: u64 = 255;

#[derive(Default)]
pub struct BoardDetail {
    pub(super) cap: u64,
    pub(super) smooth: u64,
    pub(super) bright: u64,
    bright_heard: Option<u8>,
    name: String,
    pub(super) wifi_name: String,
    pub(super) wifi_password: String,
    pub(super) words: HashMap<&'static str, String>,
    pay_to: String,
    pay_amount: String,
}

pub fn tell(cx: &Cx, id: BoardId, action: BoardAction) {
    cx.send(Command::Board { id, action });
}

/// A row that turns something on or off. It is a switch once the board says which it is, and two
/// words on a firmware that does not. The slot keeps what follows in place when the answer arrives.
pub(super) fn switch_row(
    ui: &mut Ui,
    cx: &Cx,
    id: BoardId,
    keys: (&str, &str),
    known: Option<bool>,
    make: fn(bool) -> BoardAction,
) {
    let t = cx.t;
    row(ui, t, cx.say(keys.0), cx.say(keys.1), |ui| {
        slot(ui, |ui| match known {
            Some(mut on) => {
                if switch(ui, t, &mut on, cx.say(keys.0)).clicked() {
                    tell(cx, id, make(on));
                }
            }
            None => {
                for (key, on) in [("common.on", true), ("common.off", false)] {
                    if button(ui, t, cx.say(key), Kind::Plain).clicked() {
                        tell(cx, id, make(on));
                    }
                }
            }
        });
    });
}

impl BoardDetail {
    pub fn select(&mut self, cx: &Cx, board: &BoardView) {
        let beacon = &board.seen.beacon;
        self.cap = u64::from(beacon.limit);
        self.smooth = match beacon.smooth {
            0 => u64::from(BOARD_SCREEN_RATE),
            fps => u64::from(fps),
        };
        self.bright = board.state.bright.map_or(u64::from(BOARD_BRIGHT), u64::from);
        self.bright_heard = board.state.bright;
        cx.send(Command::ReadBoardState(board.seen.id));
        self.name.clone_from(&beacon.name);
    }

    fn name_row(&mut self, ui: &mut Ui, cx: &Cx, id: BoardId) {
        let t = cx.t;
        row(ui, t, cx.say("boards.name"), cx.say("boards.name_about"), |ui| {
            let changed = field(ui, &mut self.name, cx.say("boards.name"), 170.0);
            let enter = changed.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            let ready = !self.name.trim().is_empty();
            let save = button_on(ui, t, cx.say("common.save"), Kind::Plain, ready).clicked();
            if ready && (enter || save) {
                tell(cx, id, BoardAction::Name(self.name.trim().to_string()));
            }
        });
    }

    fn mining_row(ui: &mut Ui, cx: &Cx, board: &BoardView) {
        let t = cx.t;
        let id = board.seen.id;
        let beacon = &board.seen.beacon;
        let line = format::rate(cx.voice(), beacon.hashrate.into());
        row(ui, t, cx.say("boards.mining"), &line, |ui| {
            let mut on = beacon.mining;
            if switch(ui, t, &mut on, cx.say("boards.mining")).clicked() {
                tell(cx, id, BoardAction::Mine(on));
            }
        });
    }

    fn bright_row(&mut self, ui: &mut Ui, cx: &Cx, id: BoardId, told: Option<u8>) {
        let t = cx.t;
        // The board answers after the page has opened, and a number being typed stays.
        if told != self.bright_heard {
            self.bright_heard = told;
            self.bright = told.map_or(self.bright, u64::from);
        }
        let names = ["boards.dim", "boards.middle", "boards.bright_full"];
        let items: Vec<(u64, String)> =
            BRIGHT_STEPS.iter().zip(names).map(|(step, key)| (*step, cx.say(key).to_string())).collect();
        row(ui, t, cx.say("boards.bright"), cx.say("boards.bright_about"), |ui| {
            let near = BRIGHT_STEPS.iter().copied().min_by_key(|step| step.abs_diff(self.bright));
            if let Some(level) = segments(ui, t, &items, near) {
                self.bright = level;
                tell(cx, id, BoardAction::Bright(level));
            }
            if cx.expert() {
                number_field(ui, &mut self.bright, cx.say("boards.bright"), 64.0);
                if button(ui, t, cx.say("common.apply"), Kind::Plain).clicked() {
                    self.bright = self.bright.min(BRIGHT_MAX);
                    tell(cx, id, BoardAction::Bright(self.bright));
                }
            }
        });
    }

    fn update_row(ui: &mut Ui, cx: &mut Cx, board: &BoardView) {
        let t = cx.t;
        let id = board.seen.id;
        let build = board.seen.beacon.build.clone();
        let about = cx.fill("boards.update_about", &[&build]);
        row(ui, t, cx.say("boards.update"), &about, |ui| {
            match &board.job {
                Some(job) => {
                    meta(ui, super::boards::job_text(cx, job), t);
                }
                None => {
                    if button(ui, t, cx.say("boards.update"), Kind::Plain).clicked() {
                        cx.send(Command::UpdateBoard(id));
                    }
                }
            }
        });
        if cx.settings.auto_update {
            row(ui, t, cx.say("boards.auto_update"), cx.say("boards.auto_update_about"), |ui| {
                let mut picked = cx.settings.auto_boards.contains(&id);
                if switch(ui, t, &mut picked, cx.say("boards.auto_update")).clicked() {
                    cx.settings.auto_boards.retain(|board| *board != id);
                    if picked {
                        cx.settings.auto_boards.push(id);
                    }
                    cx.save();
                }
            });
        }
    }

    fn screen_rows(ui: &mut Ui, cx: &Cx, board: &BoardView) {
        let t = cx.t;
        let id = board.seen.id;
        let beacon = &board.seen.beacon;
        row(ui, t, cx.say("boards.screen"), cx.say("boards.screen_about"), |ui| {
            let items = [
                ("easy", cx.say("boards.view_easy").to_string()),
                ("basic", cx.say("boards.view_basic").to_string()),
                ("detail", cx.say("boards.view_detail").to_string()),
            ];
            let now = items.iter().map(|item| item.0).find(|word| *word == beacon.view);
            if let Some(word) = segments(ui, t, &items, now) {
                tell(cx, id, BoardAction::View(word.into()));
            }
        });
        row(ui, t, cx.say("boards.language"), cx.say("boards.language_about"), |ui| {
            let items = [("en", "English".to_string()), ("ko", "한국어".to_string())];
            let now = items.iter().map(|item| item.0).find(|word| *word == beacon.language);
            if let Some(word) = segments(ui, t, &items, now) {
                tell(cx, id, BoardAction::Language(word.into()));
            }
        });
    }

    fn wifi_row(&mut self, ui: &mut Ui, cx: &Cx, board: &BoardView) {
        let t = cx.t;
        let id = board.seen.id;
        let joined = board.state.on_wifi;
        let about = if joined == Some(true) { "boards.home_wifi_joined" } else { "boards.home_wifi_about" };
        row(ui, t, cx.say("boards.home_wifi"), cx.say(about), |ui| {
            field(ui, &mut self.wifi_name, cx.say("boards.wifi_name"), 130.0);
            secret_field(ui, &mut self.wifi_password, cx.say("boards.wifi_password"), 110.0);
            let ready = !self.wifi_name.trim().is_empty();
            if button_on(ui, t, cx.say("boards.connect"), Kind::Plain, ready).clicked() {
                let name = self.wifi_name.trim().to_string();
                let password = std::mem::take(&mut self.wifi_password);
                tell(cx, id, BoardAction::Wifi { name, password });
            }
            if button(ui, t, cx.say("boards.disconnect"), Kind::Quiet).clicked() {
                tell(cx, id, BoardAction::WifiOff);
            }
        });
    }

    fn connection_rows(&mut self, ui: &mut Ui, cx: &Cx, board: &BoardView) {
        let t = cx.t;
        let id = board.seen.id;
        let beacon = &board.seen.beacon;
        let state = &board.state;
        let keys = ("boards.nearby", "boards.nearby_about");
        switch_row(ui, cx, id, keys, state.nearby, BoardAction::EspNow);
        let keys = ("boards.bluetooth", "boards.bluetooth_about");
        switch_row(ui, cx, id, keys, state.bluetooth, BoardAction::Bluetooth);
        let keys = ("boards.wifi", "boards.wifi_about");
        switch_row(ui, cx, id, keys, state.own_network, BoardAction::OwnNetwork);
        self.wifi_row(ui, cx, board);
        row(ui, t, cx.say("boards.alone"), cx.say("boards.alone_about"), |ui| {
            let mut offline = beacon.offline;
            if switch(ui, t, &mut offline, cx.say("boards.alone")).clicked() {
                tell(cx, id, BoardAction::Offline(offline));
            }
        });
    }

    fn money_rows(&mut self, ui: &mut Ui, cx: &mut Cx, board: &BoardView) {
        let t = cx.t;
        let id = board.seen.id;
        let beacon = &board.seen.beacon;
        let mut slip = None;
        if beacon.genesis != cx.snap.node.genesis {
            let blocks = format::number(beacon.height.into());
            row(ui, t, cx.say("boards.other_coin"), cx.say("boards.other_coin_about"), |ui| {
                if button(ui, t, cx.say("boards.move"), Kind::Plain).clicked() {
                    slip = Some(cx.slip_for(Slipping {
                        what: cx.say("boards.move"),
                        rows: vec![
                            (cx.say("boards.name"), beacon.name.clone()),
                            (cx.say("boards.blocks_go"), blocks.clone()),
                        ],
                        warning: cx.say("boards.move_warning"),
                        sign: cx.say("boards.move_yes"),
                        danger: true,
                        order: vec![Command::MoveBoard(id)],
                    }));
                }
            });
        }
        row(ui, t, cx.say("boards.mine_to"), cx.say("boards.mine_to_about"), |ui| {
            let mine = cx.snap.node.address.clone();
            if button(ui, t, cx.say("boards.mine_to_me"), Kind::Plain).clicked() {
                tell(cx, id, BoardAction::MineTo(mine));
            }
        });
        row(ui, t, cx.say("boards.pay"), cx.say("boards.pay_about"), |ui| {
            field(ui, &mut self.pay_to, cx.say("send.to_hint"), 150.0);
            field(ui, &mut self.pay_amount, cx.say("send.amount_hint"), 70.0);
            let ready = !self.pay_to.trim().is_empty() && !self.pay_amount.trim().is_empty();
            if button_on(ui, t, cx.say("send.next"), Kind::Plain, ready).clicked() {
                let (to, amount) = (self.pay_to.trim().to_string(), self.pay_amount.trim().to_string());
                slip = Some(cx.slip_for(Slipping {
                    what: cx.say("boards.pay"),
                    rows: vec![
                        (cx.say("boards.name"), beacon.name.clone()),
                        (cx.say("send.to"), to.clone()),
                        (cx.say("send.amount"), amount.clone()),
                    ],
                    warning: cx.say("send.warning"),
                    sign: cx.say("send.sign"),
                    danger: false,
                    order: vec![Command::Board { id, action: BoardAction::Pay { to, amount } }],
                }));
                self.pay_amount.clear();
            }
        });
        if slip.is_some() {
            *cx.slip = slip;
        }
    }

    pub fn show(&mut self, ui: &mut Ui, cx: &mut Cx, board: &BoardView) {
        let t = cx.t;
        let id = board.seen.id;
        ui.spacing_mut().item_spacing.y = 0.0;
        let way = if board.returning { String::new() } else { reach_text(cx, &board.reach) };
        row(ui, t, cx.say("boards.connection"), cx.say("boards.connection_about"), |ui| {
            ui.label(way);
        });
        if !board.seen.online || board.returning {
            // Every other row is an order to the board, and it cannot hear one now.
            row(ui, t, cx.say("boards.state"), cx.say("boards.gone_about"), |ui| {
                let line = board.job.as_deref().map_or(cx.say("boards.gone"), |job| super::boards::job_text(cx, job));
                ui.label(line);
            });
            return;
        }
        self.name_row(ui, cx, id);
        Self::mining_row(ui, cx, board);
        self.bright_row(ui, cx, id, board.state.bright);
        Self::update_row(ui, cx, board);
        Self::screen_rows(ui, cx, board);
        group(ui, cx.say("boards.connections"));
        self.connection_rows(ui, cx, board);
        group(ui, cx.say("boards.money"));
        self.money_rows(ui, cx, board);
        if cx.expert() {
            expert_rows(self, ui, cx, board);
        }
        ui.add_space(tokens::GAP_WIDE);
    }
}
