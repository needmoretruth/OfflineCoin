//! Experiments, apart from everything else. They run with the tools outside the coin, on boards
//! a person hands over, and the coin knows nothing about them.
use super::{Cx, Page};
use crate::core::Command;
use crate::i18n::lab_line;
use crate::ui::tokens;
use crate::ui::widgets::{self, button, button_on, card, meta, number_field, section, Kind};
use crate::ui::Icon;
use egui::{ComboBox, Ui};
use olc_board::Args;
use olc_control::BoardId;

#[derive(Default)]
pub struct Experiments {
    attacker: Option<BoardId>,
    victim: Option<BoardId>,
    amount: Option<u64>,
    confirmations: Option<u64>,
}

const AMOUNT: u64 = 10;
const CONFIRMATIONS: u64 = 2;

fn pick(ui: &mut Ui, cx: &Cx, label: &str, slot: &mut Option<BoardId>) {
    let online = cx.snap.boards.iter().filter(|board| board.seen.online);
    let name_of = |id| {
        cx.snap
            .boards
            .iter()
            .find(|board| Some(board.seen.id) == id)
            .map_or(String::new(), |board| board.seen.beacon.name.clone())
    };
    ui.label(label);
    ComboBox::from_id_salt(label)
        .selected_text(name_of(*slot))
        .show_ui(ui, |ui| {
            for board in online {
                let id = board.seen.id;
                ui.selectable_value(slot, Some(id), board.seen.beacon.name.clone());
            }
        });
}

impl Page for Experiments {
    fn title(&self) -> &'static str {
        "page.experiments"
    }

    fn icon(&self) -> Icon {
        Icon::Experiments
    }

    fn apart(&self) -> bool {
        true
    }

    fn show(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        widgets::title(ui, cx.say("page.experiments"));
        let online = cx.snap.boards.iter().filter(|board| board.seen.online).count();
        let running = cx.snap.experiment.running;
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
                pick(ui, cx, cx.say("lab.attacker"), &mut self.attacker);
                pick(ui, cx, cx.say("lab.victim"), &mut self.victim);
                let mut amount = self.amount.unwrap_or(AMOUNT);
                ui.label(cx.say("lab.amount"));
                number_field(ui, &mut amount, 70.0);
                self.amount = Some(amount);
                let mut confirmations = self.confirmations.unwrap_or(CONFIRMATIONS);
                ui.label(cx.say("lab.confirmations"));
                number_field(ui, &mut confirmations, 60.0);
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
                        cx.send(Command::StartExperiment {
                            boards: vec![attacker, victim],
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
