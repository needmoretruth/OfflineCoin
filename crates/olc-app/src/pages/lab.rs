//! Experiments, apart from everything else. The Lab's own runs start plain miners on this
//! computer, on a coin of their own, so a person sees a 51% attack or a network split happen with
//! nothing but this computer. Boards stay an extra, in expert mode, under the runs.
use super::lab_boards::BoardLab;
use super::lab_view::{self, Act, Seen};
use super::Cx;
use crate::local::lab::{has_kept, remove_kept, LabChoice, LabRun};
use crate::ui::tokens;
use crate::ui::widgets::section;
use egui::Ui;
use std::time::Duration;

/// How often a running Lab draws again, so blocks appear as they are found.
const REDRAW: Duration = Duration::from_millis(300);

#[derive(Default)]
pub struct Lab {
    run: Option<LabRun>,
    choice: LabChoice,
    boards: BoardLab,
}

impl Lab {
    pub fn show(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        let state = self.run.as_ref().map(LabRun::state);
        let running = self.run.as_ref().is_some_and(LabRun::running);
        if running {
            ui.ctx().request_repaint_after(REDRAW);
        }
        let seen = Seen { state: state.as_ref(), running, kept: has_kept() };
        match lab_view::draw(ui, t, cx.voice(), &mut self.choice, &seen) {
            Some(Act::Start) if !running => self.run = Some(LabRun::start(self.choice.clone())),
            Some(Act::Stop) => {
                if let Some(run) = &self.run {
                    run.stop();
                }
            }
            Some(Act::RemoveKept) => remove_kept(),
            _ => {}
        }
        if cx.expert() {
            ui.add_space(tokens::GAP_WIDE);
            section(ui, cx.say("lab2.boards"));
            self.boards.show(ui, cx);
        }
    }
}
