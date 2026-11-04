//! Joining a pool that runs on another computer, for any miner here. The same rows stand on the
//! pool page, on each miner on this computer, and in the mining settings, so a person finds them
//! wherever they look for the miner.
use super::pool::label_of;
use super::Cx;
use crate::core::{computer_name, pool_address, worker_part, Choice, Command, Join, Link, LinkRow, PoolOrder, PoolView, Who};
use crate::ui::explain;
use crate::ui::format;
use crate::ui::tokens;
use crate::ui::widgets::{self, button, button_on, card, dot, field, meta, section, switch_row, Kind};
use egui::{Color32, RichText, Ui};
use olc_wallet::format_address;

const ADDRESS_WIDTH: f32 = 220.0;
const USER_WIDTH: f32 = 420.0;
/// Wide enough for a whole address and a name, so nobody signs in with a name they cannot see.
const NAME_WIDTH: f32 = 620.0;
/// Lines under a switch start where its words do.
const INDENT: f32 = 44.0;

/// What is being typed for one miner, kept with the window between frames.
#[derive(Clone, Default)]
struct Draft {
    open: bool,
    address: String,
    user: String,
}

/// The name a miner signs in with when nobody wrote one: where its coins go, then its own name.
fn first_user(cx: &Cx, who: Who) -> String {
    let snap = cx.snap;
    let (address, name) = match who {
        Who::Own => (snap.node.miner.clone(), computer_name()),
        Who::Node(id) => {
            let view = snap.local.iter().find(|view| view.spec.id == id);
            let address = view.and_then(|view| view.status.as_ref()).map(|status| status.address.clone());
            (address.unwrap_or_default(), view.map(|view| view.spec.name.clone()).unwrap_or_default())
        }
        Who::Board(id) => {
            let board = snap.boards.iter().find(|board| board.seen.id == id);
            let address = board.map(|board| format_address(&board.seen.beacon.address));
            (address.unwrap_or_default(), board.map(|board| board.seen.beacon.name.clone()).unwrap_or_default())
        }
        Who::Lent(address) => {
            let choice = snap.pool.choices.iter().find(|choice| choice.who == Some(who));
            (format_address(&address), choice.map(|choice| choice.label.clone()).unwrap_or_default())
        }
    };
    format!("{address}.{}", worker_part(&name))
}

/// How a joined miner stands: mining in a pool here or elsewhere, still dialling, or what went
/// wrong, with the colour of its light.
pub fn standing_words(cx: &Cx, link: &LinkRow) -> (Color32, String) {
    let t = cx.t;
    match (link.trouble, link.view.link) {
        (Some(key), _) => (t.bad, cx.say(key).to_string()),
        (None, Link::Working) if link.here => (t.good, cx.say("pool.in_pool_here").to_string()),
        (None, Link::Working) => (t.good, cx.fill("pool.mining_in", &[&link.join.address])),
        (None, _) => (t.warn, cx.say("pool.dialling").to_string()),
    }
}

/// How a joined miner stands, in one line with a light.
fn standing(ui: &mut Ui, cx: &Cx, link: &LinkRow) {
    let t = cx.t;
    let (colour, said) = standing_words(cx, link);
    ui.horizontal_wrapped(|ui| {
        dot(ui, colour);
        ui.label(said);
        let counts = cx.fill("pool.link_counts", &[&format::number(link.view.accepted.into()), &format::number(link.view.refused.into())]);
        meta(ui, &counts, t);
        if cx.expert() && link.view.difficulty > 0 {
            meta(ui, &cx.fill("pro.pool_share_at", &[&format::number(link.view.difficulty.into())]), t);
        }
    });
}

/// The pool rows of one miner: how it stands in a pool, or a way to join one.
pub fn rows(ui: &mut Ui, cx: &Cx, who: Who) {
    let t = cx.t;
    let view = &cx.snap.pool;
    widgets::slot(ui, |ui| {
        if let Some(link) = view.link_of(who) {
            standing(ui, cx, link);
            if !link.here && button(ui, t, cx.say("pool.leave"), Kind::Plain).clicked() {
                cx.send(Command::Pool(PoolOrder::Leave(who)));
            }
            return;
        }
        let id = ui.make_persistent_id(("pool-join", who));
        let mut draft: Draft = ui.data_mut(|data| data.get_temp(id)).unwrap_or_default();
        if !draft.open {
            if button(ui, t, cx.say("pool.join_open"), Kind::Plain).clicked() {
                draft.open = true;
                draft.user = first_user(cx, who);
            }
        } else {
            form(ui, cx, who, &mut draft, view);
        }
        ui.data_mut(|data| data.insert_temp(id, draft));
    });
}

fn form(ui: &mut Ui, cx: &Cx, who: Who, draft: &mut Draft, view: &PoolView) {
    let t = cx.t;
    ui.horizontal_wrapped(|ui| {
        ui.vertical(|ui| {
            meta(ui, cx.say("pool.address"), t);
            field(ui, &mut draft.address, "192.168.0.5:3333", ADDRESS_WIDTH);
        });
        ui.vertical(|ui| {
            meta(ui, cx.say("pool.user"), t);
            field(ui, &mut draft.user, cx.say("pool.user"), USER_WIDTH);
        });
    });
    meta(ui, cx.say("pool.user_about"), t);
    let address = pool_address(&draft.address);
    ui.horizontal(|ui| {
        let ready = address.is_some() && !draft.user.trim().is_empty();
        if button_on(ui, t, cx.say("pool.join"), Kind::Main, ready).clicked() {
            if let Some(address) = address {
                let join = Join { who, address, user: draft.user.trim().to_string() };
                cx.send(Command::Pool(PoolOrder::Join(join)));
                draft.open = false;
            }
        }
        if button(ui, t, cx.say("common.cancel"), Kind::Quiet).clicked() {
            draft.open = false;
        }
    });
    let wrong = view.error.as_ref().map(|(key, _)| *key).filter(|key| matches!(*key, "pool.error_address" | "pool.error_user"));
    if let Some(key) = wrong {
        ui.colored_label(t.bad, cx.say(key));
    }
}

/// The pool page's card for pools on other computers. Every miner here that joined a pool elsewhere stands first, each with its shares and Leave.
/// Below, one form joins any of the others: this computer, its miners and every board this app
/// knows, a board heard only by radio through the board on the cable, each with its own name.
pub fn page_card(ui: &mut Ui, cx: &Cx, view: &PoolView) {
    let t = cx.t;
    card(ui, t, |ui| {
        section(ui, cx.say("pool.join_title"));
        explain::short(ui, t, cx.say("pool.join_about"));
        explain::details(ui, t, "pool-join-how", (cx.say("explain.details"), cx.say("explain.hide")), |ui| {
            ui.label(cx.say("pool.join_boards_about"));
        });
        let elsewhere: Vec<&LinkRow> = view.links.iter().filter(|link| !link.here).collect();
        if !elsewhere.is_empty() {
            ui.add_space(tokens::GAP);
            // In columns, so the counts and the Leave buttons line up under each other.
            egui::Grid::new("pool-joined").num_columns(4).spacing([tokens::GAP, 6.0]).show(ui, |ui| {
                elsewhere.iter().for_each(|link| joined_row(ui, cx, link));
            });
        }
        ui.add_space(tokens::GAP);
        let id = ui.make_persistent_id("pool-join-many");
        let mut draft: Many = ui.data_mut(|data| data.get_temp(id)).unwrap_or_default();
        let free: Vec<&Choice> = view.choices.iter().filter(|choice| choice.who.is_none_or(|who| view.link_of(who).is_none_or(|link| link.here))).collect();
        if !draft.open {
            if !free.is_empty() && button(ui, t, cx.say("pool.join_open"), Kind::Plain).clicked() {
                draft = Many { open: true, ..Many::default() };
                // This computer is chosen first, as the one most people bring.
                if free.iter().any(|choice| choice.who == Some(Who::Own)) {
                    draft.chosen.push((Who::Own, first_user(cx, Who::Own)));
                }
            }
        } else {
            many_form(ui, cx, &mut draft, &free, view);
        }
        ui.data_mut(|data| data.insert_temp(id, draft));
    });
}

/// One miner working in a pool elsewhere, as a row of the grid: its name, how it stands, its
/// shares and Leave.
fn joined_row(ui: &mut Ui, cx: &Cx, link: &LinkRow) {
    let t = cx.t;
    let (colour, said) = standing_words(cx, link);
    ui.horizontal(|ui| {
        dot(ui, colour);
        ui.label(RichText::new(label_of(cx, Some(link.join.who), &link.label)).strong());
    });
    ui.label(said);
    ui.horizontal(|ui| {
        let counts = cx.fill("pool.link_counts", &[&format::number(link.view.accepted.into()), &format::number(link.view.refused.into())]);
        meta(ui, &counts, t);
        if cx.expert() && link.view.difficulty > 0 {
            meta(ui, &cx.fill("pro.pool_share_at", &[&format::number(link.view.difficulty.into())]), t);
        }
    });
    if button(ui, t, cx.say("pool.leave"), Kind::Plain).clicked() {
        cx.send(Command::Pool(PoolOrder::Leave(link.join.who)));
    }
    ui.end_row();
}

/// The form that joins many miners at once, kept with the window between frames.
#[derive(Clone, Default)]
struct Many {
    open: bool,
    address: String,
    /// Each miner switched on, with the name it signs in with.
    chosen: Vec<(Who, String)>,
}

fn many_form(ui: &mut Ui, cx: &Cx, draft: &mut Many, free: &[&Choice], view: &PoolView) {
    let t = cx.t;
    meta(ui, cx.say("pool.address"), t);
    field(ui, &mut draft.address, "192.168.0.5:3333", ADDRESS_WIDTH);
    ui.add_space(tokens::GAP);
    section(ui, cx.say("pool.join_who"));
    widgets::slot(ui, |ui| {
        for choice in free {
            join_row(ui, cx, choice, draft);
        }
    });
    if !draft.chosen.is_empty() {
        meta(ui, cx.say("pool.user_about"), t);
    }
    let address = pool_address(&draft.address);
    ui.add_space(tokens::GAP);
    ui.horizontal(|ui| {
        let named = draft.chosen.iter().all(|(_, user)| !user.trim().is_empty());
        let ready = address.is_some() && !draft.chosen.is_empty() && named;
        if button_on(ui, t, cx.say("pool.join"), Kind::Main, ready).clicked() {
            if let Some(address) = address {
                for (who, user) in std::mem::take(&mut draft.chosen) {
                    let join = Join { who, address: address.clone(), user: user.trim().to_string() };
                    cx.send(Command::Pool(PoolOrder::Join(join)));
                }
                draft.open = false;
            }
        }
        if button(ui, t, cx.say("common.cancel"), Kind::Quiet).clicked() {
            draft.open = false;
        }
        if draft.chosen.is_empty() {
            meta(ui, cx.say("pool.join_choose"), t);
        }
    });
    let wrong = view.error.as_ref().map(|(key, _)| *key).filter(|key| matches!(*key, "pool.error_address" | "pool.error_user"));
    if let Some(key) = wrong {
        ui.colored_label(t.bad, cx.say(key));
    }
}

/// One miner in the form: a switch and, once on, the name it signs in with; a board heard by
/// radio says which board it comes through, and one that cannot take pool work says why.
fn join_row(ui: &mut Ui, cx: &Cx, choice: &Choice, draft: &mut Many) {
    let t = cx.t;
    let name = label_of(cx, choice.who, &choice.label);
    ui.horizontal_wrapped(|ui| {
        match (choice.who, choice.why_not) {
            (Some(who), None) => {
                let mut on = draft.chosen.iter().any(|(known, _)| *known == who);
                if switch_row(ui, t, &mut on, &name) {
                    draft.chosen.retain(|(known, _)| *known != who);
                    if on {
                        draft.chosen.push((who, first_user(cx, who)));
                    }
                }
            }
            _ => {
                let mut off = false;
                ui.add_enabled_ui(false, |ui| switch_row(ui, t, &mut off, &name));
            }
        }
        if choice.speed > 0 {
            meta(ui, &format::rate(cx.voice(), choice.speed), t);
        }
        if let Some(through) = &choice.through {
            meta(ui, &cx.fill("pool.through", &[through]), t);
        }
    });
    if let Some(why) = choice.why_not {
        ui.horizontal(|ui| {
            ui.add_space(INDENT);
            ui.label(RichText::new(cx.say(why)).size(tokens::META).color(t.warn));
        });
    }
    let user = draft.chosen.iter_mut().find(|(known, _)| Some(*known) == choice.who).map(|(_, user)| user);
    if let Some(user) = user {
        ui.horizontal(|ui| {
            ui.add_space(INDENT);
            field(ui, user, cx.say("pool.user"), NAME_WIDTH);
        });
    }
}
