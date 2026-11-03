//! This computer's mining in one card: on or off, how fast it may go, how its speed went over ten
//! minutes, how far the next block has come and how many blocks it found. Home and Mining show the
//! same card.
use super::speed::Speed;
use super::Cx;
use crate::core::{unix_ms, Command};
use crate::ui::chain::{self, Look};
use crate::ui::tokens;
use crate::ui::widgets::{button, button_on, card, dot, section, Kind};
use crate::ui::format;
use egui::{Align, Label, Layout, RichText, Ui};

/// The speed line's height on the card.
const LINE_HEIGHT: f32 = 80.0;

#[derive(Default)]
pub struct MiningCard {
    speed: Speed,
}

/// Mining, catching up or resting, in one line with a light.
pub fn state_line(ui: &mut Ui, cx: &Cx) {
    let t = cx.t;
    let node = &cx.snap.node;
    let voice = cx.voice();
    ui.horizontal(|ui| {
        if let Some(target) = node.syncing_to {
            dot(ui, t.warn);
            let height = format::number(node.height.into());
            ui.add(Label::new(cx.fill("home.catching_up", &[&height, &format::number(target.into())])).wrap());
        } else if node.mining {
            dot(ui, t.good);
            let speed = format::rate(voice, node.hashrate);
            let line = match node.seconds_per_block() {
                Some(seconds) => cx.fill("home.mining_block", &[&speed, &format::seconds(voice, seconds)]),
                None => cx.fill("home.mining", &[&speed]),
            };
            ui.add(Label::new(line).wrap());
        } else {
            dot(ui, t.muted);
            ui.label(RichText::new(cx.say("home.not_mining")).color(t.soft));
        }
    });
}

/// Start or stop, as one button that says what it will do.
pub fn start_stop(ui: &mut Ui, cx: &Cx) {
    let t = cx.t;
    let node = &cx.snap.node;
    if node.mining {
        if button(ui, t, cx.say("home.stop"), Kind::Plain).clicked() {
            cx.send(Command::SetMining(false));
        }
    } else if button_on(ui, t, cx.say("home.start"), Kind::Main, node.light.is_none()).clicked() {
        cx.send(Command::SetMining(true));
    }
}

impl MiningCard {
    /// `heading` names the card; `salt` keeps two cards' controls apart.
    pub fn show(&mut self, ui: &mut Ui, cx: &mut Cx, heading: &str, salt: &str, still: bool) {
        let t = *cx.t;
        card(ui, &t, |ui| {
            ui.horizontal(|ui| {
                section(ui, heading);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| start_stop(ui, cx));
            });
            state_line(ui, cx);
            ui.add_space(tokens::GAP);
            self.speed.show(ui, cx, salt);
            ui.add_space(tokens::GAP_WIDE);
            let ticker = cx.ticker().to_string();
            let look = Look { t: &t, voice: cx.voice(), still, now_ms: unix_ms(), ticker: &ticker };
            let node = &cx.snap.node;
            chain::hashrate_line(ui, &look, &node.chain, LINE_HEIGHT);
            ui.add_space(tokens::GAP);
            chain::next_block_bar(ui, &look, &node.chain);
            ui.add_space(tokens::GAP);
            facts(ui, cx);
        });
    }
}

/// What this computer found and who it mines beside, in one row. A count of none is left out
/// where most people have none.
fn facts(ui: &mut Ui, cx: &Cx) {
    let t = cx.t;
    let snap = cx.snap;
    let mut shown = vec![
        ("home.found", snap.node.blocks_mined.into()),
        ("home.others", snap.node.peers as u128),
    ];
    if !snap.local.is_empty() {
        shown.push(("home.miners_here", snap.local.len() as u128));
    }
    if !snap.boards.is_empty() {
        shown.push(("home.boards", snap.boards.len() as u128));
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (key, count) in shown {
            ui.add(Label::new(RichText::new(cx.say(key)).size(tokens::META).color(t.muted)).selectable(false));
            ui.label(RichText::new(format::number(count)).size(tokens::META).color(t.text));
            ui.add_space(tokens::GAP_WIDE);
        }
    });
}
