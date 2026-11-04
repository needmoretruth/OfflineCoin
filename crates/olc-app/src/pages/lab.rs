//! The Lab: one map where a person builds a small network on this computer by clicking. Each
//! "+ kind" adds one real participant, a node process of its own on the Lab's coin; the setups add
//! several at once, and the attack and the split run on them with the steps, words and pictures
//! the Lab always showed. The person's chain and wallet are never part of it. Boards stay an extra,
//! in expert mode, under the map. Leaving the page stops every participant.
use super::lab_boards::BoardLab;
use super::lab_chain;
use super::lab_events;
use super::lab_map::{self, MapAct, MapView};
use super::lab_palette;
use super::lab_panel::{self, Held};
use super::lab_view::{self, Act};
use super::Cx;
use crate::i18n::{fill, text, Voice};
use crate::local::lab_net::{Kind, LabNet, NetState, Order, Room};
use crate::ui::explain;
use crate::ui::tokens::{self, Tokens};
use crate::ui::widgets::{card, section};
use egui::{RichText, Ui};
use std::time::{Duration, Instant};

/// How often the Lab draws again while its network runs, so blocks show as they are found.
const REDRAW: Duration = Duration::from_millis(250);
const ROOM_EVERY: Duration = Duration::from_secs(2);
/// Below this width the panel goes under the map instead of beside it.
const SIDE_BY_SIDE: f32 = 860.0;
const PANEL_WIDTH: f32 = 300.0;
const MAP_HEIGHT: f32 = 340.0;

#[derive(Default)]
pub struct Lab {
    net: Option<LabNet>,
    map: MapView,
    said: lab_events::Said,
    held: Held,
    /// What this computer carries before the network starts, measured now and then.
    room: Option<(Instant, Room)>,
    boards: BoardLab,
    /// The person's speed limit as last seen: everything OLC Core runs stays under it.
    cap: u64,
}

impl Lab {
    /// A network starts under the person's speed limit, which holds the Lab's miners together.
    fn order(&mut self, order: Order) {
        let cap = self.cap;
        let net = self.net.get_or_insert_with(|| {
            let net = LabNet::start();
            net.order(Order::Budget(cap));
            net
        });
        net.order(order);
    }

    /// Every participant stops: the page was left.
    pub fn leave(&mut self) {
        self.net = None;
        self.map = MapView::default();
    }

    fn state(&mut self) -> NetState {
        if let Some(net) = &self.net {
            return net.state();
        }
        let room = match self.room {
            Some((at, room)) if at.elapsed() < ROOM_EVERY => room,
            _ => {
                let room = Room::measure(0);
                self.room = Some((Instant::now(), room));
                room
            }
        };
        NetState { room, ..NetState::default() }
    }

    pub fn show(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let (t, voice) = (cx.t, cx.voice());
        self.cap = cx.settings.hash_cap;
        let state = self.state();
        if self.net.is_some() {
            ui.ctx().request_repaint_after(REDRAW);
            if state.budget != self.cap {
                self.order(Order::Budget(self.cap));
            }
        }
        explain::short(ui, t, text(voice, "net.intro"));
        ui.add_space(tokens::GAP);
        let hashing = (state.budget > 0).then(|| {
            let asked: u64 = state.members.iter().filter(|member| member.kind.mines() && !member.paused).map(|member| member.speed).sum();
            (asked, state.budget)
        });
        let mut asked = card(ui, t, |ui| lab_palette::draw(ui, t, voice, &state.room, state.members.len(), hashing));
        ui.add_space(tokens::GAP);
        asked = asked.or(card(ui, t, |ui| self.map_and_panel(ui, t, voice, &state)));
        if let Some(error) = &state.error {
            let words = if error.starts_with("net.") { text(voice, error).to_string() } else { fill(text(voice, "net.failed"), &[error]) };
            ui.label(RichText::new(words).color(t.bad));
        }
        if !state.members.is_empty() {
            ui.add_space(tokens::GAP);
            asked = asked.or(card(ui, t, |ui| lab_chain::draw(ui, t, voice, &state)));
        }
        if let Some(experiment) = &state.experiment {
            ui.add_space(tokens::GAP);
            match lab_view::draw(ui, t, voice, experiment) {
                Some(Act::Stop) => asked = Some(Order::StopExperiment),
                Some(Act::Close) => asked = Some(Order::Forget),
                None => {}
            }
        }
        if let Some(order) = asked {
            if let Order::Remove(id) = order {
                if self.map.selected == Some(id) {
                    self.map.selected = None;
                }
            }
            if order == Order::Clear {
                self.map.selected = None;
            }
            self.order(order);
        }
        if cx.expert() {
            ui.add_space(tokens::GAP_WIDE);
            section(ui, cx.say("lab2.boards"));
            self.boards.show(ui, cx);
        }
    }

    /// The map, and beside it or under it the panel of the token clicked.
    fn map_and_panel(&mut self, ui: &mut Ui, t: &Tokens, voice: Voice, state: &NetState) -> Option<Order> {
        let selected = self.map.selected.and_then(|id| state.members.iter().find(|member| member.id == id));
        if self.map.selected.is_some() && selected.is_none() {
            self.map.selected = None;
        }
        let mut order = None;
        let mut map_act = None;
        let wide = ui.available_width() >= SIDE_BY_SIDE;
        match (selected, wide) {
            (Some(member), true) => {
                ui.horizontal_top(|ui| {
                    let map_width = ui.available_width() - PANEL_WIDTH - tokens::GAP_WIDE;
                    ui.vertical(|ui| {
                        ui.set_width(map_width);
                        map_act = lab_map::draw(ui, t, voice, &mut self.map, state, MAP_HEIGHT);
                        lab_events::draw(ui, t, voice, &mut self.said, state);
                    });
                    ui.add_space(tokens::GAP);
                    ui.vertical(|ui| {
                        ui.set_width(PANEL_WIDTH);
                        order = lab_panel::draw(ui, t, voice, &mut self.held, member, state);
                    });
                });
            }
            (Some(member), false) => {
                map_act = lab_map::draw(ui, t, voice, &mut self.map, state, MAP_HEIGHT);
                lab_events::draw(ui, t, voice, &mut self.said, state);
                ui.add_space(tokens::GAP);
                order = lab_panel::draw(ui, t, voice, &mut self.held, member, state);
            }
            (None, _) => {
                map_act = lab_map::draw(ui, t, voice, &mut self.map, state, MAP_HEIGHT);
                lab_events::draw(ui, t, voice, &mut self.said, state);
            }
        }
        let pools = state.members.iter().any(|member| member.kind == Kind::Pool);
        let alone = state.members.iter().any(|member| member.kind == Kind::Miner && member.pool.is_none());
        if pools && alone {
            ui.label(RichText::new(text(voice, "net.drag_hint")).color(t.muted).size(tokens::META));
        }
        match map_act {
            Some(MapAct::Select(id)) => self.map.selected = id,
            Some(MapAct::Join(miner, pool)) => order = Some(Order::Join(miner, pool)),
            None => {}
        }
        order
    }
}
