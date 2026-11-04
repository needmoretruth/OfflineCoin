//! This computer's mining in one card: on or off, how fast it may go, how its speed went over ten
//! minutes, how far the next block has come, how many blocks it found and the whole network beside
//! it. Home shows it; the Mining page manages every miner in full.
use super::speed::Speed;
use super::Cx;
use crate::core::{unix_ms, Command, Link, Who};
use crate::ui::chain::{self, Look};
use crate::ui::tokens;
use crate::ui::widgets::{button, button_on, card, dot, section, strong_text, Columns, Kind};
use crate::ui::format;
use egui::{Align, Label, Layout, RichText, Ui};

/// The speed line's height on the card, its ceiling's words included.
const LINE_HEIGHT: f32 = 104.0;
/// Room for every fact the card can show, so the cells keep one width as facts come and go.
const FACT_CELLS: usize = 4;

#[derive(Default)]
pub struct MiningCard {
    speed: Speed,
}

/// Mining, catching up or resting, in one line with a light.
pub fn state_line(ui: &mut Ui, cx: &Cx) {
    let t = cx.t;
    let node = &cx.snap.node;
    ui.horizontal(|ui| {
        if let Some(target) = node.syncing_to {
            dot(ui, t.warn);
            let height = format::number(node.height.into());
            ui.add(Label::new(cx.fill("home.catching_up", &[&height, &format::number(target.into())])).wrap());
        } else if node.mining {
            // No figure here: the speed line below says how fast, for every miner on this computer.
            dot(ui, t.good);
            let pooled = cx.snap.pool.link_of(Who::Own).is_some_and(|link| link.trouble.is_none() && link.view.link == Link::Working);
            ui.add(Label::new(cx.say(if pooled { "home.mining_pool" } else { "home.mining" })).wrap());
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
            chain::hashrate_line(ui, &look, &node.chain, &chain::Ceiling::of(&cx.snap.budget), LINE_HEIGHT);
            ui.add_space(tokens::GAP);
            chain::next_block_bar(ui, &look, &node.chain);
            ui.add_space(tokens::GAP);
            facts(ui, cx);
            // The whole network beside this computer, the speed now and the share the Mining page
            // shows, once there are blocks to read them from: before that they would only say
            // "unknown" and push what stops this node further down Home.
            if cx.snap.node.chain.latest.len() >= 2 {
                ui.add_space(tokens::GAP);
                super::network::figures(ui, cx, false);
            }
        });
    }
}

/// What this computer found and who it mines beside, each in its own cell: a quiet name over its
/// count. Other miners and boards show once there are some, since most people have none.
fn facts(ui: &mut Ui, cx: &Cx) {
    let t = cx.t;
    let snap = cx.snap;
    let mut shown = vec![("home.found", u128::from(snap.node.blocks_mined)), ("home.others", snap.node.peers as u128)];
    if !snap.local.is_empty() {
        shown.push(("home.miners_here", snap.local.len() as u128));
    }
    if !snap.boards.is_empty() {
        shown.push(("home.boards", snap.boards.len() as u128));
    }
    let cells = Columns::new(ui, &[1.0; FACT_CELLS]);
    cells.row(ui, |ui, index| {
        let Some((key, count)) = shown.get(index) else {
            return;
        };
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            // A long name or large text goes to a second line rather than losing its end.
            ui.add(Label::new(RichText::new(cx.say(key)).size(tokens::META).color(t.muted)).selectable(false).wrap());
            ui.label(strong_text(format::number(*count)));
        });
    });
}
