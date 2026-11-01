//! Every board this app can reach, and what can be done to one.
use super::{Cx, Page};
use crate::core::{BoardView, Command};
use crate::discovery::Reach;
use crate::ui::format;
use crate::ui::tokens;
use crate::ui::widgets::{
    self, button, card, cell, dot, field, header_cell, meta, number_field, section, segments,
    Columns, Kind,
};
use crate::ui::Icon;
use egui::{CollapsingHeader, Ui};
use olc_control::BoardId;

const WEIGHTS: [f32; 8] = [1.5, 1.4, 0.8, 1.1, 1.1, 0.5, 0.9, 0.8];
const SMOOTH_DEFAULT: u64 = 10;
const HALF_CAP: u64 = 7_500;

#[derive(Default)]
pub struct Boards {
    open: Option<BoardId>,
    cap: u64,
    smooth: u64,
    name: String,
    pay_to: String,
    pay_amount: String,
    new_name: String,
}

fn connection(cx: &Cx, board: &BoardView) -> String {
    match &board.reach {
        Reach::Usb => cx.say("reach.usb").into(),
        Reach::Radio(Some(through)) => cx.fill("reach.radio_through", &[through]),
        _ => cx.say("reach.radio").into(),
    }
}

impl Boards {
    fn select(&mut self, board: &BoardView) {
        let beacon = &board.seen.beacon;
        self.open = Some(board.seen.id);
        self.cap = u64::from(beacon.limit);
        self.smooth = SMOOTH_DEFAULT;
        self.name.clone_from(&beacon.name);
    }

    fn row(&mut self, ui: &mut Ui, cx: &Cx, columns: &Columns, board: &BoardView) {
        let t = cx.t;
        let beacon = &board.seen.beacon;
        columns.row(ui, |ui, index| match index {
            0 => {
                let colour = if board.seen.online { t.good } else { t.bad };
                dot(ui, colour);
                cell(ui, beacon.name.clone());
            }
            1 => cell(ui, connection(cx, board)),
            2 => cell(ui, format::number(beacon.height.into())),
            3 => cell(ui, format::rate(beacon.hashrate.into())),
            4 => cell(ui, format::rate(beacon.limit.into())),
            5 => cell(ui, beacon.peers.to_string()),
            6 => cell(ui, beacon.build.clone()),
            _ => {
                let open = self.open == Some(board.seen.id);
                let key = if open { "boards.close" } else { "boards.manage" };
                if button(ui, t, cx.say(key), Kind::Plain).clicked() {
                    if open {
                        self.open = None;
                    } else {
                        self.select(board);
                    }
                }
            }
        });
        if let Some(job) = &board.job {
            meta(ui, job, t);
        }
    }

    fn line(cx: &Cx, id: BoardId, line: String) {
        cx.send(Command::BoardLine { id, line });
    }

    fn actions(&mut self, ui: &mut Ui, cx: &Cx, board: &BoardView) {
        let t = cx.t;
        let id = board.seen.id;
        let beacon = &board.seen.beacon;
        ui.horizontal_wrapped(|ui| {
            section(ui, cx.say("boards.cap"));
            let items = [
                (cx.settings.board_cap, format::number(cx.settings.board_cap.into())),
                (HALF_CAP, format::number(HALF_CAP.into())),
            ];
            if let Some(cap) = segments(ui, t, &items, Some(self.cap)) {
                self.cap = cap;
                Self::line(cx, id, format!("limit {cap}"));
            }
            number_field(ui, &mut self.cap, 90.0);
            if button(ui, t, cx.say("boards.apply"), Kind::Plain).clicked() {
                Self::line(cx, id, format!("limit {}", self.cap));
            }
        });
        ui.horizontal_wrapped(|ui| {
            let offline = beacon.offline;
            let key = if offline { "boards.go_online" } else { "boards.go_offline" };
            if button(ui, t, cx.say(key), Kind::Plain).clicked() {
                let word = if offline { "off" } else { "on" };
                Self::line(cx, id, format!("offline {word}"));
            }
            let key = if beacon.mining { "boards.stop_mining" } else { "boards.start_mining" };
            if button(ui, t, cx.say(key), Kind::Plain).clicked() {
                let word = if beacon.mining { "off" } else { "on" };
                Self::line(cx, id, format!("mine {word}"));
            }
            if button(ui, t, cx.say("boards.update"), Kind::Plain).clicked() {
                cx.send(Command::UpdateBoard(id));
            }
        });
        ui.horizontal_wrapped(|ui| {
            section(ui, cx.say("boards.screen"));
            for (word, key) in [
                ("basic", "boards.view_basic"),
                ("detail", "boards.view_detail"),
                ("easy", "boards.view_easy"),
            ] {
                let kind = if beacon.view == word { Kind::Main } else { Kind::Plain };
                if button(ui, t, cx.say(key), kind).clicked() {
                    Self::line(cx, id, format!("view {word}"));
                }
            }
            ui.add_space(tokens::GAP);
            for (word, label) in [("en", "English"), ("ko", "한국어")] {
                let kind = if beacon.language == word { Kind::Main } else { Kind::Plain };
                if button(ui, t, label, kind).clicked() {
                    Self::line(cx, id, format!("lang {word}"));
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            section(ui, cx.say("boards.smooth"));
            number_field(ui, &mut self.smooth, 70.0);
            ui.label(cx.say("boards.frames_per_second"));
            if button(ui, t, cx.say("boards.apply"), Kind::Plain).clicked() {
                Self::line(cx, id, format!("smooth {}", self.smooth.max(1)));
            }
            ui.add_space(tokens::GAP);
            field(ui, &mut self.name, cx.say("boards.name"), 150.0);
            if button(ui, t, cx.say("boards.rename"), Kind::Plain).clicked() {
                Self::line(cx, id, format!("name {}", self.name.trim()));
            }
        });
        ui.horizontal_wrapped(|ui| {
            section(ui, cx.say("boards.pay"));
            field(ui, &mut self.pay_to, cx.say("wallet.to"), 300.0);
            field(ui, &mut self.pay_amount, cx.say("wallet.amount"), 110.0);
            if button(ui, t, cx.say("wallet.send"), Kind::Plain).clicked() {
                let line = format!("pay {} {}", self.pay_to.trim(), self.pay_amount.trim());
                Self::line(cx, id, line);
                self.pay_amount.clear();
            }
        });
    }

    fn install(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        if cx.snap.new_ports.is_empty() {
            return;
        }
        ui.add_space(tokens::GAP);
        card(ui, t, |ui| {
            section(ui, cx.say("boards.new_board"));
            field(ui, &mut self.new_name, cx.say("boards.name"), 200.0);
            for (port, job) in &cx.snap.new_ports {
                ui.horizontal(|ui| {
                    ui.label(port);
                    match job {
                        Some(job) => {
                            meta(ui, job, t);
                        }
                        None => {
                            let ready = !self.new_name.trim().is_empty();
                            let key = cx.say("boards.install");
                            if widgets::button_on(ui, t, key, Kind::Main, ready).clicked() {
                                cx.send(Command::InstallBoard {
                                    path: port.clone(),
                                    name: self.new_name.trim().to_string(),
                                });
                            }
                        }
                    }
                });
            }
        });
    }
}

impl Page for Boards {
    fn title(&self) -> &'static str {
        "page.boards"
    }

    fn icon(&self) -> Icon {
        Icon::Boards
    }

    fn show(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        widgets::title(ui, cx.say("page.boards"));
        if cx.snap.boards.is_empty() {
            let body = if cx.snap.usb { "boards.none_body" } else { "boards.usb_off" };
            widgets::empty(ui, t, cx.say("boards.none"), cx.say(body));
        } else {
            card(ui, t, |ui| {
                let columns = Columns::new(ui, &WEIGHTS);
                columns.row(ui, |ui, index| {
                    let keys = [
                        "column.name",
                        "column.connection",
                        "column.height",
                        "column.hash",
                        "column.cap",
                        "column.peers",
                        "column.build",
                    ];
                    if let Some(key) = keys.get(index) {
                        header_cell(ui, t, cx.say(key));
                    }
                });
                widgets::divider(ui, t);
                for board in &cx.snap.boards {
                    self.row(ui, cx, &columns, board);
                    if self.open == Some(board.seen.id) {
                        CollapsingHeader::new("")
                            .id_salt(board.seen.id)
                            .default_open(true)
                            .show_unindented(ui, |ui| self.actions(ui, cx, board));
                    }
                }
            });
        }
        self.install(ui, cx);
    }
}
