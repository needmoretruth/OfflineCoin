//! The console: the same words as the command line, for an expert, with history and completion.
//! It calls the same core functions as the buttons.
use super::reach::BOARD_ROWS;
use super::{Cx, Page};
use crate::core::WORDS;
use crate::ui::tokens;
use crate::ui::widgets::{self, button, meta, mono, secret_field, Kind};
use crate::ui::Icon;
use egui::{Align, FontId, Key, Label, Layout, ScrollArea, TextEdit, Ui};
use std::sync::mpsc::{Receiver, TryRecvError};

const SHOWN_CHOICES: usize = 12;
/// Room under the output for the line, its choices and the page margin.
const FOOT: f32 = 92.0;

enum Line {
    Asked(String),
    Said(String),
    Failed(String),
}

#[derive(Default)]
pub struct Console {
    line: String,
    password: String,
    history: Vec<String>,
    back: Option<usize>,
    lines: Vec<Line>,
    waiting: Option<Receiver<Result<String, String>>>,
}

/// Every word that could go on at this point of the line.
fn choices(cx: &Cx, line: &str) -> Vec<String> {
    let words: Vec<&str> = line.split_whitespace().collect();
    let done = if line.ends_with(' ') { words.len() } else { words.len().saturating_sub(1) };
    let last = if line.ends_with(' ') { "" } else { words.last().copied().unwrap_or("") };
    let mut all: Vec<String> = match (done, words.first().copied()) {
        (0, _) => WORDS.iter().map(|word| word.name.to_string()).chain(["help".to_string()]).collect(),
        (1, Some("board")) => cx.snap.boards.iter().map(|board| board.seen.beacon.name.clone()).collect(),
        (2, Some("board")) => BOARD_ROWS.iter().map(|row| row.word.to_string()).collect(),
        _ => Vec::new(),
    };
    all.retain(|word| word.starts_with(last) && word != last);
    all.sort();
    all.dedup();
    all
}

fn shared_start(words: &[String]) -> String {
    let Some(first) = words.first() else {
        return String::new();
    };
    let mut end = first.len();
    for word in words {
        end = end.min(first.bytes().zip(word.bytes()).take_while(|(a, b)| a == b).count());
    }
    while !first.is_char_boundary(end) {
        end -= 1;
    }
    first[..end].to_string()
}

impl Console {
    fn complete(&mut self, cx: &Cx) {
        let found = choices(cx, &self.line);
        let start = shared_start(&found);
        if start.is_empty() {
            return;
        }
        let cut = self.line.rfind(' ').map_or(0, |space| space + 1);
        self.line.truncate(cut);
        self.line.push_str(&start);
        if found.len() == 1 {
            self.line.push(' ');
        }
    }

    fn step_history(&mut self, older: bool) {
        let newest = self.history.len();
        let place = self.back.unwrap_or(newest);
        let next = match older {
            true => place.saturating_sub(1),
            false => (place + 1).min(newest),
        };
        self.back = (next < newest).then_some(next);
        self.line = self.history.get(next).cloned().unwrap_or_default();
    }

    fn run(&mut self, cx: &Cx) {
        let line = self.line.trim().to_string();
        if line.is_empty() || self.waiting.is_some() {
            return;
        }
        self.lines.push(Line::Asked(line.clone()));
        if self.history.last() != Some(&line) {
            self.history.push(line.clone());
        }
        self.back = None;
        self.line.clear();
        let password = (!self.password.is_empty()).then(|| std::mem::take(&mut self.password));
        self.waiting = Some(cx.core.console(line, password));
    }

    fn collect(&mut self, ui: &Ui) {
        let Some(waiting) = &self.waiting else {
            return;
        };
        match waiting.try_recv() {
            Ok(Ok(answer)) => {
                self.lines.push(Line::Said(answer));
                self.waiting = None;
            }
            Ok(Err(failure)) => {
                self.lines.push(Line::Failed(failure));
                self.waiting = None;
            }
            Err(TryRecvError::Disconnected) => self.waiting = None,
            Err(TryRecvError::Empty) => ui.ctx().request_repaint(),
        }
    }

    fn output(&self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        let height = (ui.available_height() - FOOT).max(120.0);
        ScrollArea::vertical()
            .id_salt("console")
            .max_height(height)
            .min_scrolled_height(height)
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                if self.lines.is_empty() {
                    meta(ui, cx.say("console.hello"), t);
                }
                for line in &self.lines {
                    let text = match line {
                        Line::Asked(text) => mono(format!("> {text}")).color(t.soft),
                        Line::Said(text) => mono(text.clone()).color(t.text),
                        Line::Failed(text) => mono(text.clone()).color(t.bad),
                    };
                    ui.add(Label::new(text).wrap());
                }
                if self.waiting.is_some() {
                    meta(ui, cx.say("console.waiting"), t);
                }
            });
    }

    fn input(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        let secret = self.line.contains("--password");
        ui.horizontal(|ui| {
            let run_width = 62.0;
            let width = ui.available_width() - if secret { 280.0 } else { run_width };
            let edit = TextEdit::singleline(&mut self.line)
                .font(FontId::monospace(tokens::TEXT))
                .hint_text(cx.say("console.hint"))
                .lock_focus(true)
                .desired_width(width)
                .id_salt("console-line");
            let line = ui.add(edit).accessible_name(cx.say("console.hint"));
            let focused = line.has_focus();
            let (enter, tab, up, down) = ui.input_mut(|input| {
                (
                    input.key_pressed(Key::Enter),
                    focused && input.consume_key(egui::Modifiers::NONE, Key::Tab),
                    focused && input.key_pressed(Key::ArrowUp),
                    focused && input.key_pressed(Key::ArrowDown),
                )
            });
            if tab {
                self.complete(cx);
            }
            if up || down {
                self.step_history(up);
            }
            if secret {
                secret_field(ui, &mut self.password, cx.say("console.password"), 180.0);
            }
            let run = button(ui, t, cx.say("console.run"), Kind::Main).clicked();
            if run || (enter && line.lost_focus()) {
                self.run(cx);
                line.request_focus();
            }
        });
        let found = choices(cx, &self.line);
        if !self.line.is_empty() && !found.is_empty() {
            let shown: Vec<&str> = found.iter().take(SHOWN_CHOICES).map(String::as_str).collect();
            meta(ui, &shown.join("  "), t);
        }
    }
}

impl Page for Console {
    fn title(&self) -> &'static str {
        "page.console"
    }

    fn icon(&self) -> Icon {
        Icon::Console
    }

    fn expert_only(&self) -> bool {
        true
    }

    fn apart(&self) -> bool {
        true
    }

    fn show(&mut self, ui: &mut Ui, cx: &mut Cx) {
        self.collect(ui);
        widgets::title(ui, cx.say("page.console"));
        ui.with_layout(Layout::top_down(Align::Min), |ui| {
            self.output(ui, cx);
            ui.add_space(tokens::GAP);
            self.input(ui, cx);
        });
    }
}
