//! The twelve seed words: shown until the person has written them down and typed two of them back,
//! then again only when asked and agreed to, as Electrum and Sparrow do. Words from elsewhere
//! bring their wallet back here, and coins from before the words can move to them.
use super::Cx;
use crate::core::{Command, LockOrder, Personal, WalletOrder};
use crate::ui::format;
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
    /// Asks before showing written words again.
    Ask,
    Again,
}

#[derive(Default)]
pub struct WordsSheet {
    step: Step,
    typed: [String; 2],
    wrong: bool,
    restore: String,
    unlock: super::wallet_seal::UnlockPrompt,
}

fn wallet(cx: &Cx, order: WalletOrder) {
    cx.send(Command::Own(Personal::Wallet(order)));
}

/// The two places to ask back. They come from the words, so they stay the same while the sheet
/// is open, and they are far apart so both halves of the list get a look.
pub(crate) fn asked(words: &[String]) -> [usize; 2] {
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

    /// Written words once more, with any this wallet had before, which still work.
    fn again(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        let view = &cx.snap.node.wallet;
        meta(ui, cx.say("words.secret"), t);
        if let Some(words) = &view.words {
            word_grid(ui, cx, &words.0);
        }
        for words in &view.earlier_words {
            ui.add_space(tokens::GAP);
            section(ui, cx.say("words.earlier"));
            meta(ui, cx.say("words.earlier_body"), t);
            word_grid(ui, cx, &words.0);
        }
        ui.add_space(4.0);
        if button(ui, t, cx.say("words.hide"), Kind::Plain).clicked() {
            self.step = Step::Show;
        }
    }

    /// Written words, shown again only after the person agrees.
    fn written(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        match self.step {
            Step::Again => self.again(ui, cx),
            Step::Ask => {
                meta(ui, cx.say("words.ask"), t);
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if button(ui, t, cx.say("words.show_go"), Kind::Main).clicked() {
                        self.step = Step::Again;
                    }
                    if button(ui, t, cx.say("common.back"), Kind::Plain).clicked() {
                        self.step = Step::Show;
                    }
                });
            }
            Step::Show | Step::Check => {
                meta(ui, cx.say("words.kept"), t);
                ui.add_space(4.0);
                if button(ui, t, cx.say("words.see_again"), Kind::Plain).clicked() {
                    self.step = Step::Ask;
                }
            }
        }
    }

    /// Coins on keys from before the words, and the one payment that moves them to the words.
    fn outside(ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        let held = cx.snap.node.wallet.outside_words;
        if held == 0 {
            return;
        }
        ui.add_space(tokens::GAP);
        section(ui, cx.say("words.move_title"));
        meta(ui, &cx.fill("words.outside", &[&format::coins(held, cx.ticker())]), t);
        let ripe = cx.snap.node.wallet.outside_ripe;
        ui.add_space(4.0);
        if button_on(ui, t, cx.say("words.move"), Kind::Main, ripe > 0).clicked() {
            cx.send(Command::Lock(LockOrder::MoveToWords));
        }
        // The line goes as the last mined coins mature. It keeps its room, so neither the button
        // nor the restore box below moves or takes another egui name just then, which lost a press.
        // egui leaves an invisible line out of what a screen reader hears.
        ui.scope(|ui| {
            let waiting = ripe < held;
            if !waiting {
                ui.set_invisible();
            }
            meta(ui, cx.say("words.outside_later"), t);
        });
    }

    /// True when it is finished.
    pub fn show(&mut self, ui: &mut Ui, cx: &Cx) -> bool {
        let t = cx.t;
        let view = &cx.snap.node.wallet;
        if let Some(words) = &view.words_to_write {
            match self.step {
                Step::Check => return self.check(ui, cx, &words.0),
                _ => return self.show_words(ui, cx, &words.0),
            }
        }
        sheet_head(ui, cx.say("words.heading"));
        // A locked wallet's words are sealed with its keys, and so is everything this sheet does.
        if super::wallet_seal::locked(cx) {
            self.unlock.show(ui, cx);
            ui.add_space(tokens::GAP);
            return button(ui, t, cx.say("common.close"), Kind::Plain).clicked();
        }
        // The words, earlier words and the coins to move can be taller than a small window: the
        // title and the close button keep their room and the rest scrolls.
        let room = (ui.ctx().content_rect().height() - 200.0).max(300.0);
        egui::ScrollArea::vertical().max_height(room).show(ui, |ui| {
            if view.has_words {
                self.written(ui, cx);
                Self::outside(ui, cx);
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
        });
        ui.add_space(tokens::GAP);
        button(ui, t, cx.say("common.close"), Kind::Plain).clicked()
    }
}
