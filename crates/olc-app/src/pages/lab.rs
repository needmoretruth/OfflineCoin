//! Experiments, apart from everything else. They run with the tools outside the coin, on boards
//! a person hands over, and the coin knows nothing about them.
use super::Cx;
use crate::core::Command;
use crate::discovery::Reach;
use crate::i18n::lab_line;
use crate::ui::tokens;
use crate::ui::widgets::{button, button_on, card, meta, number_field, pick_row, section, cell, Kind};
use crate::fleet::Args;
use egui::Ui;
use olc_control::BoardId;

#[derive(Default)]
pub struct Lab {
    tested: Option<BoardId>,
    attacker: Option<BoardId>,
    victim: Option<BoardId>,
    helper: Option<BoardId>,
    amount: Option<u64>,
    confirmations: Option<u64>,
}

const AMOUNT: u64 = 10;
const CONFIRMATIONS: u64 = 2;

/// One board out of those online, as rows.
/// With `optional`, a second click on the picked board takes it away.
fn pick(ui: &mut Ui, cx: &Cx, label: &str, slot: &mut Option<BoardId>, optional: bool) {
    ui.vertical(|ui| {
        ui.set_max_width(200.0);
        meta(ui, label, cx.t);
        for board in cx.snap.boards.iter().filter(|board| board.seen.online) {
            let id = board.seen.id;
            let name = &board.seen.beacon.name;
            let row = pick_row(ui, cx.t, 30.0, *slot == Some(id), name, |ui| cell(ui, name.as_str()));
            if row.clicked() {
                *slot = (!optional || *slot != Some(id)).then_some(id);
            }
        }
    });
}

impl Lab {
    fn cap_test(&mut self, ui: &mut Ui, cx: &Cx, running: bool) {
        let t = cx.t;
        card(ui, t, |ui| {
            section(ui, cx.say("captest.title"));
            meta(ui, cx.say("captest.about"), t);
            ui.add_space(4.0);
            let online = || cx.snap.boards.iter().filter(|board| board.seen.online);
            let on_cable = online().find(|board| board.reach == Reach::Usb);
            let first = on_cable.or_else(|| online().next());
            self.tested = self.tested.or(first.map(|board| board.seen.id));
            ui.horizontal(|ui| {
                pick(ui, cx, cx.say("captest.board"), &mut self.tested, false);
                let start = cx.say("lab.start");
                let ready = self.tested.is_some() && !running;
                if button_on(ui, t, start, Kind::Main, ready).clicked() {
                    if let Some(board) = self.tested {
                        cx.send(Command::StartCapTest(board));
                    }
                }
                if running && button(ui, t, cx.say("lab.stop"), Kind::Danger).clicked() {
                    cx.send(Command::StopExperiment);
                }
            });
        });
    }
}

impl Lab {
    pub fn show(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        let online = cx.snap.boards.iter().filter(|board| board.seen.online).count();
        let running = cx.snap.experiment.running;
        self.cap_test(ui, cx, running);
        ui.add_space(tokens::GAP);
        card(ui, t, |ui| {
            section(ui, cx.say("lab.title"));
            meta(ui, cx.say("lab.about"), t);
            ui.add_space(4.0);
            if online < 2 {
                meta(ui, cx.say("lab.two_boards"), t);
                return;
            }
            let mut boards = cx.snap.boards.iter().filter(|board| board.seen.online);
            let first = boards.next().map(|board| board.seen.id);
            let second = boards.next().map(|board| board.seen.id);
            self.attacker = self.attacker.or(first);
            self.victim = self.victim.or(second);
            ui.horizontal(|ui| {
                pick(ui, cx, cx.say("lab.attacker"), &mut self.attacker, false);
                pick(ui, cx, cx.say("lab.victim"), &mut self.victim, false);
                pick(ui, cx, cx.say("lab.helper"), &mut self.helper, true);
                let mut amount = self.amount.unwrap_or(AMOUNT);
                ui.label(cx.say("lab.amount"));
                number_field(ui, &mut amount, cx.say("lab.amount"), 70.0);
                self.amount = Some(amount);
                let mut confirmations = self.confirmations.unwrap_or(CONFIRMATIONS);
                ui.label(cx.say("lab.confirmations"));
                number_field(ui, &mut confirmations, cx.say("lab.confirmations"), 60.0);
                self.confirmations = Some(confirmations);
            });
            ui.horizontal(|ui| {
                let pair = self.attacker.zip(self.victim).filter(|(a, b)| a != b);
                let start = cx.say("lab.start");
                if button_on(ui, t, start, Kind::Main, pair.is_some() && !running).clicked() {
                    if let Some((attacker, victim)) = pair {
                        let words = [
                            "attack".to_string(),
                            "--amount".to_string(),
                            amount_word(self.amount, AMOUNT),
                            "--confirmations".to_string(),
                            amount_word(self.confirmations, CONFIRMATIONS),
                        ];
                        let helper = self.helper.filter(|id| ![attacker, victim].contains(id));
                        let mut boards = vec![attacker, victim];
                        boards.extend(helper);
                        cx.send(Command::StartExperiment {
                            boards,
                            args: Args::parse(words.into_iter()),
                        });
                    }
                }
                if running && button(ui, t, cx.say("lab.stop"), Kind::Danger).clicked() {
                    cx.send(Command::StopExperiment);
                }
            });
        });
        let lines = &cx.snap.experiment.lines;
        if !lines.is_empty() {
            ui.add_space(tokens::GAP);
            card(ui, t, |ui| {
                for line in lines {
                    ui.label(lab_line(cx.lang, line));
                }
            });
        }
    }
}

fn amount_word(value: Option<u64>, default: u64) -> String {
    value.unwrap_or(default).to_string()
}
