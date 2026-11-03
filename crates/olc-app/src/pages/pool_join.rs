//! Joining a pool that runs on another computer, for any miner here. The same rows stand on the
//! pool page, on each miner on this computer, and in the mining settings, so a person finds them
//! wherever they look for the miner.
use super::pool::label_of;
use super::Cx;
use crate::core::{computer_name, pool_address, worker_part, Command, Join, Link, LinkRow, PoolOrder, PoolView, Who};
use crate::ui::explain;
use crate::ui::format;
use crate::ui::tokens;
use crate::ui::widgets::{self, button, button_on, card, dot, field, meta, section, Kind};
use egui::{RichText, Ui};
use olc_wallet::format_address;

const ADDRESS_WIDTH: f32 = 220.0;
const USER_WIDTH: f32 = 420.0;

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
    };
    format!("{address}.{}", worker_part(&name))
}

/// How a joined miner stands, in one line with a light.
fn standing(ui: &mut Ui, cx: &Cx, link: &LinkRow) {
    let t = cx.t;
    let (colour, said) = match (link.trouble, link.view.link) {
        (Some(key), _) => (t.bad, cx.say(key).to_string()),
        (None, Link::Working) if link.here => (t.good, cx.say("pool.in_pool_here").to_string()),
        (None, Link::Working) => (t.good, cx.fill("pool.mining_in", &[&link.join.address])),
        (None, _) => (t.warn, cx.say("pool.dialling").to_string()),
    };
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

/// The pool page's card: this computer joins a pool elsewhere, and how every miner here that
/// joined one stands.
pub fn page_card(ui: &mut Ui, cx: &Cx, view: &PoolView) {
    let t = cx.t;
    card(ui, t, |ui| {
        section(ui, cx.say("pool.join_title"));
        explain::short(ui, t, cx.say("pool.join_about"));
        ui.add_space(tokens::GAP);
        ui.label(RichText::new(cx.say("pool.this_computer")).strong());
        rows(ui, cx, Who::Own);
        for link in view.links.iter().filter(|link| link.join.who != Who::Own && !link.here) {
            ui.add_space(tokens::GAP);
            ui.label(RichText::new(label_of(cx, Some(link.join.who), &link.label)).strong());
            rows(ui, cx, link.join.who);
        }
    });
}
