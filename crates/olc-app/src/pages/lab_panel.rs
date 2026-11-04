//! The panel of the token a person clicked: what it is and does in at most three lines, and at
//! most three controls: its speed (a stop and three presets, and a slider), where a miner mines
//! (alone or in a pool) or one action of its own, and remove.
use super::lab_glyphs as glyphs;
use super::lab_tokens::{facts, name};
use crate::i18n::{fill, text, Voice};
use crate::local::lab_net::{Kind, Member, NetState, Order, Phase};
use crate::ui::tokens::{self, Tokens};
use crate::ui::widgets::{button_on, segments, slider, Kind as ButtonKind};
use egui::{vec2, RichText, Sense, Ui};

/// The slider's place while the hand moves it, so the order goes once it lets go.
#[derive(Default)]
pub struct Held {
    speed: Option<(u32, f32)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Preset {
    Stop,
    At(usize),
}

fn speed_key(kind: Kind) -> &'static str {
    match kind {
        Kind::Sender => "net.speed_pay",
        Kind::Spammer => "net.speed_spam",
        _ => "net.speed_hash",
    }
}

fn speed(ui: &mut Ui, t: &Tokens, voice: Voice, held: &mut Held, member: &Member, enabled: bool) -> Option<Order> {
    let range = member.kind.speed()?;
    let mut order = None;
    ui.horizontal(|ui| {
        ui.label(RichText::new(text(voice, "net.speed")).color(t.soft));
        let shown = if member.paused { text(voice, "net.paused").to_string() } else { fill(text(voice, speed_key(member.kind)), &[&member.speed.to_string()]) };
        ui.label(RichText::new(shown).strong().color(t.text));
    });
    let presets = [
        (Preset::Stop, text(voice, "net.preset_stop").to_string()),
        (Preset::At(0), text(voice, "net.preset_slow").to_string()),
        (Preset::At(1), text(voice, "net.preset_normal").to_string()),
        (Preset::At(2), text(voice, "net.preset_fast").to_string()),
    ];
    let current = if member.paused { Some(Preset::Stop) } else { range.presets.iter().position(|preset| *preset == member.speed).map(Preset::At) };
    ui.add_enabled_ui(enabled, |ui| {
        match segments(ui, t, &presets, current) {
            Some(Preset::Stop) => order = Some(Order::Pause(member.id, true)),
            // After a stop, a preset also starts it again.
            Some(Preset::At(at)) => {
                order = Some(Order::Speed(member.id, range.presets[at]));
                held.speed = Some((member.id, range.presets[at] as f32));
            }
            None => {}
        }
        let mut value = match held.speed {
            Some((id, value)) if id == member.id => value,
            _ => member.speed as f32,
        };
        let slid = slider(ui, t, &mut value, range.least as f32..=range.most as f32, text(voice, "net.speed"), &[]);
        held.speed = Some((member.id, value));
        if slid.settled {
            order = Some(Order::Speed(member.id, value.round() as u64));
        }
    });
    order
}

/// Where a miner mines: alone, or in one of the pools.
fn where_it_mines(ui: &mut Ui, t: &Tokens, voice: Voice, member: &Member, state: &NetState, enabled: bool) -> Option<Order> {
    let pools: Vec<&Member> = state.members.iter().filter(|other| other.kind == Kind::Pool).collect();
    if pools.is_empty() {
        return None;
    }
    ui.label(RichText::new(text(voice, "net.where")).color(t.soft));
    let mut choices = vec![(None, text(voice, "net.alone").to_string())];
    choices.extend(pools.iter().map(|pool| (Some(pool.id), name(voice, pool))));
    let mut order = None;
    ui.add_enabled_ui(enabled, |ui| {
        if let Some(chosen) = segments(ui, t, &choices, Some(member.pool)) {
            order = Some(Order::Join(member.id, chosen));
        }
    });
    order
}

/// The one action of its own a kind has, beside speed and remove.
fn action(ui: &mut Ui, t: &Tokens, voice: Voice, member: &Member, state: &NetState, enabled: bool) -> Option<Order> {
    let (key, shown) = match member.kind {
        Kind::Attacker => {
            let ended = state.experiment.as_ref().is_some_and(|run| matches!(run.phase, Phase::Done | Phase::Failed));
            ("net.again_attack", ended && !member.busy)
        }
        Kind::Exploiter => ("net.again_tricks", member.waits.is_none() || member.waits == Some("net.tricks_not_yet")),
        Kind::Full | Kind::Pruned | Kind::Light | Kind::Receiver => {
            let key = if member.paused { "net.reconnect" } else { "net.cut" };
            let clicked = button_on(ui, t, text(voice, key), ButtonKind::Plain, enabled).clicked();
            return clicked.then_some(Order::Pause(member.id, !member.paused));
        }
        _ => return None,
    };
    let clicked = shown && button_on(ui, t, text(voice, key), ButtonKind::Plain, enabled).clicked();
    clicked.then_some(Order::Again(member.id))
}

/// The panel. Returns what was pressed.
pub fn draw(ui: &mut Ui, t: &Tokens, voice: Voice, held: &mut Held, member: &Member, state: &NetState) -> Option<Order> {
    let enabled = !member.busy;
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::hover());
        ui.painter().circle_filled(rect.center(), 15.0, glyphs::fill(t, member.kind));
        glyphs::paint(ui.painter(), member.kind, rect.center(), 18.0, glyphs::ink(t, member.kind));
        ui.label(RichText::new(name(voice, member)).strong().size(tokens::SECTION).color(t.text));
    });
    ui.add_space(4.0);
    for line in facts(voice, member, state) {
        ui.label(RichText::new(line).color(t.soft));
    }
    if member.busy {
        ui.label(RichText::new(text(voice, "net.busy")).color(t.warn));
    }
    ui.add_space(tokens::GAP);
    let mut order = speed(ui, t, voice, held, member, enabled);
    if member.kind == Kind::Miner {
        ui.add_space(tokens::GAP);
        order = order.or(where_it_mines(ui, t, voice, member, state, enabled));
    }
    ui.add_space(tokens::GAP);
    let below = ui.horizontal_wrapped(|ui| {
        let own = action(ui, t, voice, member, state, enabled);
        let remove = button_on(ui, t, text(voice, "net.remove"), ButtonKind::Plain, true).clicked();
        if remove { Some(Order::Remove(member.id)) } else { own }
    });
    order.or(below.inner)
}
