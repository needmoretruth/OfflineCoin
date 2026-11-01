//! Every board this app can reach, and what can be done to one.
use super::{Cx, Page};
use crate::core::{BoardAction, BoardView, Command};
use crate::discovery::Reach;
use crate::ui::format;
use crate::ui::tokens;
use crate::ui::widgets::{
    self, button, card, cell, dot, field, header_cell, meta, number_field, section, segments,
    switch_row, Columns, Kind,
};
use crate::ui::Icon;
use egui::Ui;
use olc_control::defaults::BOARD_SCREEN_RATE;
use olc_control::BoardId;

const WEIGHTS: [f32; 8] = [1.5, 1.4, 0.8, 1.1, 1.1, 0.5, 0.9, 0.8];
/// Used only until a board says its own number.
const BRIGHT_FALLBACK: u64 = 64;
const BRIGHT_MAX: u64 = 255;
const LABEL_WIDTH: f32 = 150.0;

#[derive(Default)]
pub struct Boards {
    open: Option<BoardId>,
    cap: u64,
    smooth: u64,
    bright: Option<u64>,
    name: String,
    pay_to: String,
    pay_amount: String,
    new_name: String,
    mine_to: String,
    /// The board asked to change coin, waiting for a yes.
    moving: Option<BoardId>,
}

/// One action row, its name in a column of its own so the controls line up.
fn labelled(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal_wrapped(|ui| {
        let area = egui::vec2(LABEL_WIDTH, tokens::CONTROL);
        let left = egui::Layout::left_to_right(egui::Align::Center);
        ui.allocate_ui_with_layout(area, left, |ui| {
            ui.set_width(area.x);
            section(ui, label);
        });
        add(ui);
    });
}

fn connection(cx: &Cx, board: &BoardView) -> String {
    super::network::reach_text(cx, &board.reach)
}

/// A job is a word from the manager, or how far an update got.
fn job_text<'a>(cx: &Cx, job: &'a str) -> &'a str {
    match job {
        "preparing" => cx.say("boards.job_preparing"),
        "restarting" => cx.say("boards.job_restarting"),
        "installing" => cx.say("boards.job_installing"),
        "update_failed" => cx.say("boards.job_failed"),
        progress => progress,
    }
}

impl Boards {
    fn select(&mut self, cx: &Cx, board: &BoardView) {
        let beacon = &board.seen.beacon;
        self.open = Some(board.seen.id);
        self.cap = u64::from(beacon.limit);
        self.smooth = match beacon.smooth {
            0 => u64::from(BOARD_SCREEN_RATE),
            fps => u64::from(fps),
        };
        self.bright = None;
        cx.send(Command::ReadBright(board.seen.id));
        self.name.clone_from(&beacon.name);
    }

    fn row(&mut self, ui: &mut Ui, cx: &Cx, columns: &Columns, board: &BoardView) {
        let t = cx.t;
        let beacon = &board.seen.beacon;
        columns.row(ui, |ui, index| match index {
            0 => {
                let colour = match (board.seen.online, board.returning) {
                    (true, _) => t.good,
                    (false, true) => t.muted,
                    (false, false) => t.bad,
                };
                dot(ui, colour);
                cell(ui, beacon.name.clone());
            }
            // Nothing is known of how a restarting board will come back.
            1 if board.returning => cell(ui, String::new()),
            1 => cell(ui, connection(cx, board)),
            2 => cell(ui, format::number(beacon.height.into())),
            3 => cell(ui, format::rate(beacon.hashrate.into())),
            4 => match beacon.limit {
                0 => cell(ui, cx.say("boards.no_cap")),
                limit => cell(ui, format::rate(limit.into())),
            },
            5 => cell(ui, beacon.peers.to_string()),
            6 => cell(ui, beacon.build.clone()),
            _ => {
                let open = self.open == Some(board.seen.id);
                let key = if open { "boards.close" } else { "boards.manage" };
                if button(ui, t, cx.say(key), Kind::Plain).clicked() {
                    if open {
                        self.open = None;
                    } else {
                        self.select(cx, board);
                    }
                }
            }
        });
        if let Some(job) = &board.job {
            meta(ui, job_text(cx, job), t);
        }
    }

    fn tell(cx: &Cx, id: BoardId, action: BoardAction) {
        cx.send(Command::Board { id, action });
    }

    fn pick_for_updates(ui: &mut Ui, cx: &mut Cx, id: BoardId) {
        let mut picked = cx.settings.auto_boards.contains(&id);
        if switch_row(ui, cx.t, &mut picked, cx.say("boards.auto_update")) {
            cx.settings.auto_boards.retain(|board| *board != id);
            if picked {
                cx.settings.auto_boards.push(id);
            }
            cx.save();
        }
    }

    fn actions(&mut self, ui: &mut Ui, cx: &Cx, board: &BoardView) {
        let t = cx.t;
        let id = board.seen.id;
        let beacon = &board.seen.beacon;
        labelled(ui, cx.say("boards.cap"), |ui| {
            let items = [(cx.settings.board_cap, format::number(cx.settings.board_cap.into()))];
            if let Some(cap) = segments(ui, t, &items, Some(self.cap)) {
                self.cap = cap;
                Self::tell(cx, id, BoardAction::Cap(cap));
            }
            let full = beacon.limit == 0;
            let key = if full { "boards.full_off" } else { "boards.full_on" };
            if button(ui, t, cx.say(key), Kind::Plain)
                .on_hover_text(cx.say("boards.full_note"))
                .clicked()
            {
                Self::tell(cx, id, BoardAction::FullSpeed(!full));
            }
            number_field(ui, &mut self.cap, 90.0);
            if button(ui, t, cx.say("boards.apply"), Kind::Plain).clicked() {
                Self::tell(cx, id, BoardAction::Cap(self.cap));
            }
        });
        labelled(ui, cx.say("boards.state"), |ui| {
            let offline = beacon.offline;
            let key = if offline { "boards.go_online" } else { "boards.go_offline" };
            if button(ui, t, cx.say(key), Kind::Plain).clicked() {
                Self::tell(cx, id, BoardAction::Offline(!offline));
            }
            let key = if beacon.mining { "boards.stop_mining" } else { "boards.start_mining" };
            if button(ui, t, cx.say(key), Kind::Plain).clicked() {
                Self::tell(cx, id, BoardAction::Mine(!beacon.mining));
            }
            if button(ui, t, cx.say("boards.update"), Kind::Plain).clicked() {
                cx.send(Command::UpdateBoard(id));
            }
        });
        labelled(ui, cx.say("boards.screen"), |ui| {
            for (word, key) in [
                ("basic", "boards.view_basic"),
                ("detail", "boards.view_detail"),
                ("easy", "boards.view_easy"),
            ] {
                let kind = if beacon.view == word { Kind::Main } else { Kind::Plain };
                if button(ui, t, cx.say(key), kind).clicked() {
                    Self::tell(cx, id, BoardAction::View(word.into()));
                }
            }
            ui.add_space(tokens::GAP);
            for (word, label) in [("en", "English"), ("ko", "한국어")] {
                let kind = if beacon.language == word { Kind::Main } else { Kind::Plain };
                if button(ui, t, label, kind).clicked() {
                    Self::tell(cx, id, BoardAction::Language(word.into()));
                }
            }
        });
        labelled(ui, cx.say("boards.smooth"), |ui| {
            number_field(ui, &mut self.smooth, 70.0);
            let (lowest, highest) = beacon.smooth_range;
            let range = [lowest.to_string(), highest.to_string()];
            meta(ui, &cx.fill("boards.frames_per_second", &[&range[0], &range[1]]), t);
            if button(ui, t, cx.say("boards.apply"), Kind::Plain).clicked() {
                self.smooth = self.smooth.clamp(u64::from(lowest), u64::from(highest));
                Self::tell(cx, id, BoardAction::Smooth(self.smooth));
            }
        });
        labelled(ui, cx.say("boards.bright"), |ui| {
            let said = board.bright.map_or(BRIGHT_FALLBACK, u64::from);
            let mut level = self.bright.unwrap_or(said);
            if number_field(ui, &mut level, 70.0) {
                self.bright = Some(level);
            }
            meta(ui, &cx.fill("boards.bright_range", &[&BRIGHT_MAX.to_string()]), t);
            if button(ui, t, cx.say("boards.apply"), Kind::Plain).clicked() {
                let level = level.min(BRIGHT_MAX);
                self.bright = Some(level);
                Self::tell(cx, id, BoardAction::Bright(level));
            }
        });
        labelled(ui, cx.say("boards.wifi"), |ui| {
            for (key, on) in [("boards.wifi_on", true), ("boards.wifi_off", false)] {
                if button(ui, t, cx.say(key), Kind::Plain).clicked() {
                    Self::tell(cx, id, BoardAction::OwnNetwork(on));
                }
            }
            meta(ui, cx.say("boards.wifi_note"), t);
        });
        labelled(ui, cx.say("boards.bluetooth"), |ui| {
            for (key, on) in [("boards.wifi_on", true), ("boards.wifi_off", false)] {
                if button(ui, t, cx.say(key), Kind::Plain).clicked() {
                    Self::tell(cx, id, BoardAction::Bluetooth(on));
                }
            }
            meta(ui, cx.say("boards.bluetooth_note"), t);
        });
        labelled(ui, cx.say("boards.name"), |ui| {
            field(ui, &mut self.name, cx.say("boards.name"), 200.0);
            if button(ui, t, cx.say("boards.rename"), Kind::Plain).clicked() {
                Self::tell(cx, id, BoardAction::Name(self.name.clone()));
            }
        });
        labelled(ui, cx.say("boards.mine_to"), |ui| {
            field(ui, &mut self.mine_to, cx.say("mining.pay_to_hint"), 300.0);
            if button(ui, t, cx.say("mining.set"), Kind::Plain).clicked() {
                let action = match self.mine_to.trim() {
                    "" => BoardAction::MineToOwnKey,
                    target => BoardAction::MineTo(target.into()),
                };
                Self::tell(cx, id, action);
            }
            let mine = cx.snap.node.address.clone();
            if button(ui, t, cx.say("boards.mine_to_me"), Kind::Quiet).clicked() {
                self.mine_to.clone_from(&mine);
                Self::tell(cx, id, BoardAction::MineTo(mine));
            }
        });
        if beacon.genesis != cx.snap.node.genesis {
            labelled(ui, cx.say("boards.other_coin"), |ui| {
                if self.moving == Some(id) {
                    let blocks = format::number(beacon.height.into());
                    meta(ui, &cx.fill("boards.move_warning", &[&blocks]), t);
                    if button(ui, t, cx.say("boards.move_yes"), Kind::Main).clicked() {
                        cx.send(Command::MoveBoard(id));
                        self.moving = None;
                    }
                } else if button(ui, t, cx.say("boards.move"), Kind::Plain).clicked() {
                    self.moving = Some(id);
                }
            });
        }
        labelled(ui, cx.say("boards.pay"), |ui| {
            field(ui, &mut self.pay_to, cx.say("wallet.to"), 300.0);
            field(ui, &mut self.pay_amount, cx.say("wallet.amount"), 110.0);
            if button(ui, t, cx.say("wallet.send"), Kind::Plain).clicked() {
                let to = self.pay_to.clone();
                let amount = self.pay_amount.clone();
                Self::tell(cx, id, BoardAction::Pay { to, amount });
                self.pay_amount.clear();
            }
        });
    }

    fn install(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        if cx.snap.new_ports.is_empty() {
            return;
        }
        ui.add_space(tokens::GAP);
        card(ui, t, |ui| {
            section(ui, cx.say("boards.new_board"));
            field(ui, &mut self.new_name, cx.say("boards.name"), 200.0);
            for (port, job) in &cx.snap.new_ports {
                ui.horizontal(|ui| {
                    ui.label(port);
                    match job {
                        Some(job) => {
                            meta(ui, job_text(cx, job), t);
                        }
                        None => {
                            let ready = !self.new_name.trim().is_empty();
                            let key = cx.say("boards.install");
                            if widgets::button_on(ui, t, key, Kind::Main, ready).clicked() {
                                cx.send(Command::InstallBoard {
                                    path: port.clone(),
                                    name: self.new_name.trim().to_string(),
                                });
                            }
                        }
                    }
                });
            }
        });
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
        if cx.snap.boards.is_empty() {
            let body = if cx.snap.usb { "boards.none_body" } else { "boards.usb_off" };
            widgets::empty(ui, t, cx.say("boards.none"), cx.say(body));
        } else {
            card(ui, t, |ui| {
                let columns = Columns::new(ui, &WEIGHTS);
                columns.row(ui, |ui, index| {
                    let keys = [
                        "column.name",
                        "column.connection",
                        "column.height",
                        "column.hash",
                        "column.cap",
                        "column.peers",
                        "column.build",
                    ];
                    if let Some(key) = keys.get(index) {
                        header_cell(ui, t, cx.say(key));
                    }
                });
                widgets::divider(ui, t);
                for board in &cx.snap.boards {
                    self.row(ui, cx, &columns, board);
                    if self.open == Some(board.seen.id) {
                        ui.add_space(4.0);
                        widgets::divider(ui, t);
                        ui.add_space(tokens::GAP);
                        self.actions(ui, cx, board);
                        if cx.settings.auto_update {
                            Self::pick_for_updates(ui, cx, board.seen.id);
                        }
                        ui.add_space(tokens::GAP);
                        widgets::divider(ui, t);
                    }
                }
            });
        }
        self.install(ui, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Core, CoreLaunch, Snapshot};
    use crate::discovery::Reach;
    use crate::fleet::{Connection, Seen};
    use crate::i18n::Lang;
    use crate::settings::{Paths, Settings};
    use crate::ui::tokens::DARK;
    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;
    use olc_control::Beacon;
    use std::path::PathBuf;
    use std::time::Duration;

    const BOARD: BoardId = [4; 6];

    type Scene = (Boards, Snapshot, Settings, Core);

    fn scene(online: bool, returning: bool) -> Scene {
        let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.scratch/tests/board");
        let settings = Settings {
            setup_done: true,
            scan_lan: false,
            broadcast: false,
            port: 7310,
            ..Settings::default()
        };
        let launch = CoreLaunch {
            paths: Paths { root: folder },
            fleet: None,
            usb: false,
            node_program: None,
        };
        let core = Core::start(launch, settings.clone()).unwrap();
        let beacon = Beacon { name: "board".into(), limit: 1_000, ..Beacon::default() };
        let seen = Seen {
            id: BOARD,
            beacon,
            connection: Connection::Direct { link: 0 },
            online,
            silent_for: Duration::ZERO,
            old_format: false,
        };
        let job = returning.then(|| "restarting".to_string());
        let view =
            BoardView { seen, job, answer: None, returning, reach: Reach::Usb, bright: None };
        let boards = Boards { open: Some(BOARD), ..Boards::default() };
        (boards, Snapshot { boards: vec![view], usb: true, ..Snapshot::default() }, settings, core)
    }

    fn drawn(state: Scene) -> Harness<'static, Scene> {
        let mut harness = Harness::builder().with_size(egui::vec2(1280.0, 900.0)).build_ui_state(
            |ui, (page, snap, settings, core): &mut Scene| {
                let mut cx = Cx { lang: Lang::En, snap, settings, t: &DARK, core };
                page.show(ui, &mut cx);
            },
            state,
        );
        harness.run_steps(3);
        harness
    }

    #[test]
    fn a_restarting_board_says_so_and_offers_the_full_speed_setting() {
        let mut harness = drawn(scene(false, true));
        assert!(harness.query_by_label_contains("Restarting").is_some());
        assert!(harness.query_by_label_contains("Radio").is_none());
        assert!(harness.query_by_label_contains("Full speed").is_some());
        assert!(harness.query_by_label_contains("7,500").is_none());
        if let Some(folder) = std::env::var_os("OLC_SHOTS").map(PathBuf::from) {
            let image = harness.render().expect("a renderer");
            image.save(folder.join("board-restarting.png")).expect("save");
        }
    }
}
