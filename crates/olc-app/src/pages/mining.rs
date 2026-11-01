//! Mining with this computer, up to a number a person chooses.
use super::{Cx, Page};
use crate::core::Command;
use crate::settings::RELAXED_CAP;
use crate::ui::format;
use crate::ui::tokens;
use crate::ui::widgets::{self, card, meta, number_field, section, segments, stats, switch_row};
use crate::ui::Icon;
use egui::Ui;

#[derive(Default)]
pub struct Mining;

impl Page for Mining {
    fn title(&self) -> &'static str {
        "page.mining"
    }

    fn icon(&self) -> Icon {
        Icon::Mining
    }

    fn show(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        let node = &cx.snap.node;
        widgets::title(ui, cx.say("page.mining"));
        card(ui, t, |ui| {
            let mut on = node.mining;
            if switch_row(ui, t, &mut on, cx.say("mining.mine_here")) {
                cx.send(Command::SetMining(on));
            }
            if let Some(target) = node.syncing_to {
                let text = cx.fill("mining.catching_up", &[&format::number(target.into())]);
                meta(ui, &text, t);
            }
            ui.add_space(tokens::GAP);
            let cap = match node.cap {
                0 => cx.say("mining.no_cap").to_string(),
                cap => format::rate(cap),
            };
            stats(
                ui,
                t,
                &[
                    (cx.say("mining.measured"), format::rate(node.hashrate)),
                    (cx.say("mining.cap"), cap),
                    (cx.say("mining.blocks_mined"), format::number(node.blocks_mined.into())),
                    (cx.say("mining.difficulty"), format::number(node.difficulty.into())),
                    (cx.say("mining.height"), format::number(node.height.into())),
                ],
            );
        });
        ui.add_space(tokens::GAP);
        card(ui, t, |ui| {
            section(ui, cx.say("mining.cap"));
            let half = cx.snap.full_rate / 2;
            let items = [
                (RELAXED_CAP, cx.say("mining.relaxed").to_string()),
                (half, cx.say("mining.half").to_string()),
                (0, cx.say("mining.full").to_string()),
            ];
            let mut cap = node.cap;
            if let Some(chosen) = segments(ui, t, &items, Some(cap)) {
                cx.send(Command::SetCap(chosen));
            }
            ui.horizontal(|ui| {
                if number_field(ui, &mut cap, 120.0) {
                    cx.send(Command::SetCap(cap));
                }
                ui.label("H/s");
            });
            let full = format::rate(cx.snap.full_rate);
            meta(ui, &cx.fill("mining.full_rate", &[&full]), t);
        });
    }
}
