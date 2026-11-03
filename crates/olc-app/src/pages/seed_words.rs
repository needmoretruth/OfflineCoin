//! The twelve seed words: shown until the person has written them down and typed two of them back,
//! then never again. Words from elsewhere bring their wallet back here.
use super::Cx;
use crate::core::{Command, Personal, WalletOrder};
use crate::i18n::Lang;
use crate::ui::floating::sheet_head;
use crate::ui::tokens;
use crate::ui::widgets::{button, button_on, card, field, meta, section, words_box, Kind};
use egui::{Grid, Label, RichText, Ui};

#[derive(Default, PartialEq, Eq)]
enum Step {
    #[default]
    Show,
    Check,
}

#[derive(Default)]
pub struct WordsSheet {
    step: Step,
    typed: [String; 2],
    wrong: bool,
    restore: String,
}

fn wallet(cx: &Cx, order: WalletOrder) {
    cx.send(Command::Own(Personal::Wallet(order)));
}

/// The two places to ask back. They come from the words, so they stay the same while the sheet
/// is open, and they are far apart so both halves of the list get a look.
fn asked(words: &[String]) -> [usize; 2] {
    let count = words.len().max(2);
    let mix = words.iter().map(|word| word.chars().count()).sum::<usize>() + words.first().map_or(0, |word| word.len());
    let first = mix % (count / 2);
    [first, count / 2 + (mix / 3) % (count / 2)]
}

/// Twelve numbered words in three columns, the way they get written on paper.
fn word_grid(ui: &mut Ui, cx: &Cx, words: &[String]) {
    let t = cx.t;
    card(ui, t, |ui| {
        Grid::new("seed-words").num_columns(3).spacing([28.0, 10.0]).show(ui, |ui| {
            let rows = words.len().div_ceil(3);
            for row in 0..rows {
                for column in 0..3 {
                    let place = column * rows + row;
                    if let Some(word) = words.get(place) {
                        ui.horizontal(|ui| {
                            let number = RichText::new(format!("{:>2}", place + 1)).color(t.muted).monospace();
                            ui.add(Label::new(number).selectable(false));
                            ui.add(Label::new(RichText::new(word).size(tokens::TITLE)).selectable(false));
                        });
                    }
                }
                ui.end_row();
            }
        });
    });
}

impl WordsSheet {
    fn show_words(&mut self, ui: &mut Ui, cx: &Cx, words: &[String]) -> bool {
        let t = cx.t;
        sheet_head(ui, cx.fill("words.title", &[&words.len().to_string()]).as_str());
        meta(ui, cx.say("words.body"), t);
        meta(ui, cx.say("words.secret"), t);
        ui.add_space(tokens::GAP);
        word_grid(ui, cx, words);
        ui.add_space(tokens::GAP);
        let mut done = false;
        ui.horizontal(|ui| {
            if button(ui, t, cx.say("words.wrote"), Kind::Main).clicked() {
                self.step = Step::Check;
                self.wrong = false;
            }
            done = button(ui, t, cx.say("words.later"), Kind::Plain).clicked();
        });
        done
    }

    fn check(&mut self, ui: &mut Ui, cx: &Cx, words: &[String]) -> bool {
        let t = cx.t;
        sheet_head(ui, cx.say("words.check_title"));
        meta(ui, cx.say("words.check_body"), t);
        ui.add_space(tokens::GAP);
        let places = asked(words);
        let width = ui.available_width();
        for (typed, place) in self.typed.iter_mut().zip(places) {
            let hint = cx.fill("words.nth", &[&(place + 1).to_string()]);
            field(ui, typed, &hint, width);
        }
        if self.wrong {
            ui.colored_label(t.bad, cx.say("words.wrong"));
        }
        ui.add_space(tokens::GAP);
        let mut done = false;
        ui.horizontal(|ui| {
            let filled = self.typed.iter().all(|typed| !typed.trim().is_empty());
            if button_on(ui, t, cx.say("words.confirm"), Kind::Main, filled).clicked() {
                let same = |typed: &String, place: usize| words.get(place).is_some_and(|word| typed.trim().to_lowercase() == *word);
                if self.typed.iter().zip(places).all(|(typed, place)| same(typed, place)) {
                    wallet(cx, WalletOrder::WordsWritten);
                    self.typed = Default::default();
                    done = true;
                } else {
                    self.wrong = true;
                }
            }
            if button(ui, t, cx.say("words.see_again"), Kind::Plain).clicked() {
                self.step = Step::Show;
            }
        });
        done
    }

    fn restore(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        section(ui, cx.say("words.restore"));
        meta(ui, cx.say("words.restore_body"), t);
        ui.add_space(4.0);
        words_box(ui, &mut self.restore, cx.say("words.restore_hint"), ui.available_width());
        ui.add_space(4.0);
        let filled = self.restore.split_whitespace().count() >= 12;
        if button_on(ui, t, cx.say("words.restore_go"), Kind::Plain, filled).clicked() {
            wallet(cx, WalletOrder::RestoreWords(std::mem::take(&mut self.restore)));
        }
    }

    /// True when it is finished.
    pub fn show(&mut self, ui: &mut Ui, cx: &Cx) -> bool {
        let t = cx.t;
        let view = &cx.snap.node.wallet;
        if let Some(words) = &view.words_to_write {
            return match self.step {
                Step::Show => self.show_words(ui, cx, &words.0),
                Step::Check => self.check(ui, cx, &words.0),
            };
        }
        sheet_head(ui, cx.say("words.heading"));
        if view.has_words {
            meta(ui, cx.say("words.kept"), t);
        } else {
            meta(ui, cx.say("words.none"), t);
            ui.add_space(4.0);
            if button(ui, t, cx.say("words.make"), Kind::Main).clicked() {
                self.step = Step::Show;
                wallet(cx, WalletOrder::NewWords { korean: cx.lang == Lang::Ko });
            }
        }
        ui.add_space(tokens::GAP);
        self.restore(ui, cx);
        ui.add_space(tokens::GAP);
        button(ui, t, cx.say("common.close"), Kind::Plain).clicked()
    }
}
