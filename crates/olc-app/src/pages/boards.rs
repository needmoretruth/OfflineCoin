//! The hardware that mines: boards, the nodes on this computer, working together, and the lab.
use super::board_detail::BoardDetail;
use super::money::balance_of;
use super::lab::Lab;
use super::local::Local;
use super::pool::PoolTab;
use super::{Cx, Page};
use crate::core::{BoardView, Command};
use crate::ui::tokens::{self, Tokens};
use crate::ui::widgets::{self, button_on, card, dot, field, meta, pick_row, segments, Kind};
use crate::ui::{format, Icon};
use egui::{vec2, Align, Label, Layout, RichText, Ui};
use olc_control::BoardId;

const LIST_WIDTH: f32 = 248.0;
const LIST_ROW: f32 = 54.0;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Tab {
    #[default]
    Boards,
    Nodes,
    Pool,
    Lab,
}

#[derive(Default)]
pub struct Boards {
    tab: Tab,
    open: Option<BoardId>,
    detail: BoardDetail,
    new_names: Vec<(String, String)>,
    local: Local,
    pool: PoolTab,
    lab: Lab,
}

/// A job is a word from the manager, or how far an update got.
pub fn job_text<'a>(cx: &Cx, job: &'a str) -> &'a str {
    match job {
        "preparing" => cx.say("boards.job_preparing"),
        "restarting" => cx.say("boards.job_restarting"),
        "installing" => cx.say("boards.job_installing"),
        "update_failed" => cx.say("boards.job_failed"),
        progress => progress,
    }
}

fn state_colour(t: &Tokens, board: &BoardView) -> egui::Color32 {
    match (board.seen.online, board.returning) {
        (true, _) if board.seen.beacon.mining => t.good,
        (true, _) => t.info,
        (false, true) => t.muted,
        (false, false) => t.bad,
    }
}

fn list_row(ui: &mut Ui, cx: &Cx, board: &BoardView, current: bool) -> bool {
    let t = cx.t;
    let beacon = &board.seen.beacon;
    let line = match () {
        () if board.returning => cx.say("boards.restarting").to_string(),
        () if !board.seen.online => cx.say("boards.gone").to_string(),
        () if beacon.mining => format::rate(cx.voice(), beacon.hashrate.into()),
        () => cx.say("boards.resting").to_string(),
    };
    let line = format!("{line}  {}", balance_of(cx, board));
    let clicked = pick_row(ui, t, LIST_ROW, current, &beacon.name, |ui| {
        dot(ui, state_colour(t, board));
        ui.vertical(|ui| {
            ui.add_space(9.0);
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.add(Label::new(beacon.name.as_str()).truncate().selectable(false));
            ui.add(Label::new(RichText::new(line).size(tokens::META).color(t.muted)).truncate().selectable(false));
        });
    })
    .clicked();
    if let Some(job) = &board.job {
        meta(ui, job_text(cx, job), t);
    }
    clicked
}

impl Boards {
    /// A board that was plugged in and has no program yet. It comes with a name to set it up.
    fn new_ports(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        for (index, (port, job)) in cx.snap.new_ports.iter().enumerate() {
            let number = cx.snap.boards.len() + index + 1;
            let known = self.new_names.iter().position(|(seen, _)| seen == port);
            let place = known.unwrap_or_else(|| {
                let name = cx.fill("boards.new_name", &[&number.to_string()]);
                self.new_names.push((port.clone(), name));
                self.new_names.len() - 1
            });
            card(ui, t, |ui| {
                ui.horizontal(|ui| {
                    dot(ui, t.info);
                    ui.label(widgets::medium_text(cx.say("boards.new_board")));
                    if cx.expert() {
                        meta(ui, port, t);
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| match job {
                        Some(job) => {
                            meta(ui, job_text(cx, job), t);
                        }
                        None => {
                            let name = self.new_names[place].1.trim().to_string();
                            let ready = !name.is_empty();
                            let go = button_on(ui, t, cx.say("boards.install"), Kind::Main, ready);
                            field(ui, &mut self.new_names[place].1, cx.say("boards.name"), 180.0);
                            if go.clicked() {
                                cx.send(Command::InstallBoard { path: port.clone(), name });
                            }
                        }
                    });
                });
            });
            ui.add_space(tokens::GAP);
        }
    }

    fn board_list(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        let snap = cx.snap;
        if snap.boards.is_empty() {
            if snap.new_ports.is_empty() {
                let body = if snap.usb { "boards.none_body" } else { "boards.usb_off" };
                let again = cx.say("boards.look_again");
                if widgets::empty(ui, t, cx.say("boards.none"), cx.say(body), Some(again)) {
                    cx.send(Command::Rescan);
                }
            }
            return;
        }
        let known = snap.boards.iter().any(|board| Some(board.seen.id) == self.open);
        if !known {
            self.open = None;
        }
        if let Some(first) = snap.boards.first().filter(|_| self.open.is_none()) {
            self.open = Some(first.seen.id);
            self.detail.select(cx, first);
        }
        let side_by_side = ui.available_width() >= tokens::COMPACT;
        let body = |this: &mut Self, ui: &mut Ui, cx: &mut Cx| {
            if let Some(board) = snap.boards.iter().find(|board| Some(board.seen.id) == this.open) {
                this.detail.show(ui, cx, board);
            }
        };
        let list = |this: &mut Self, ui: &mut Ui, cx: &Cx| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for board in &snap.boards {
                let current = this.open == Some(board.seen.id);
                if list_row(ui, cx, board, current) {
                    this.open = Some(board.seen.id);
                    this.detail.select(cx, board);
                }
            }
        };
        if side_by_side {
            let rest = ui.available_width() - LIST_WIDTH - tokens::GAP_PAGE;
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(vec2(LIST_WIDTH, 0.0), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(LIST_WIDTH);
                    list(self, ui, cx);
                });
                ui.add_space(tokens::GAP_PAGE - ui.spacing().item_spacing.x);
                ui.allocate_ui_with_layout(vec2(rest, 0.0), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(rest);
                    body(self, ui, cx);
                });
            });
        } else {
            list(self, ui, cx);
            ui.add_space(tokens::GAP_WIDE);
            body(self, ui, cx);
        }
    }
}

impl Page for Boards {
    fn title(&self) -> &'static str {
        "page.boards"
    }

    fn icon(&self) -> Icon {
        Icon::Boards
    }

    fn show(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        widgets::title(ui, cx.say("page.boards"));
        let mut tabs = vec![
            (Tab::Boards, cx.say("boards.tab_boards").to_string()),
            (Tab::Nodes, cx.say("boards.tab_nodes").to_string()),
            (Tab::Pool, cx.say("boards.tab_pool").to_string()),
        ];
        if cx.expert() {
            tabs.push((Tab::Lab, cx.say("pro.tab_lab").to_string()));
        } else if self.tab == Tab::Lab {
            self.tab = Tab::Boards;
        }
        if let Some(tab) = segments(ui, t, &tabs, Some(self.tab)) {
            self.tab = tab;
        }
        ui.add_space(tokens::GAP_WIDE - tokens::GAP);
        match self.tab {
            Tab::Boards => {
                self.new_ports(ui, cx);
                self.board_list(ui, cx);
            }
            Tab::Nodes => self.local.show(ui, cx),
            Tab::Pool => self.pool.show(ui, cx),
            Tab::Lab => self.lab.show(ui, cx),
        }
    }
}
