//! The wallet: everything that happened to its coins, and its tools grouped by what a person wants
//! to do: keep it safe, receive and prove, share with others, and the rest. Expert mode adds locks.
//! Each tool says in one line what it is for and opens in a sheet.
use super::ledger::Ledger;
use super::locks::Locks;
use super::wallet_tools::{Tool, WalletTools};
use super::{Cx, Page};
use crate::ui::explain;
use crate::ui::theme::desktop_reduces_motion;
use crate::ui::tokens;
use crate::ui::widgets::{self, button, card, meta, section, Kind};
use crate::ui::Icon;
use egui::{vec2, Align, Label, Layout, RichText, Ui};

/// Every row the wallet keeps.
const ALL_ROWS: usize = 200;
/// The tools' column beside the rows.
const TOOLS_WIDTH: f32 = 400.0;

/// What a row of the tools opens.
#[derive(Clone, Copy)]
enum Opens {
    Tool(Tool),
    Locks,
}

/// A tool: its name, the line under it and the word on its button.
struct Row {
    keys: (&'static str, &'static str, &'static str),
    opens: Opens,
}

const fn row(name: &'static str, about: &'static str, go: &'static str, opens: Opens) -> Row {
    Row { keys: (name, about, go), opens }
}

/// The groups in order, each with its heading. Expert rows come last in their group.
fn groups(expert: bool) -> Vec<(&'static str, Vec<Row>)> {
    let mut more = vec![
        row("settings.watch", "settings.watch_about", "common.open", Opens::Tool(Tool::Watch)),
        row("settings.offline", "settings.offline_about", "common.open", Opens::Tool(Tool::Offline)),
    ];
    if expert {
        more.push(row("page.locks", "pro.locks_about", "common.open", Opens::Locks));
    }
    vec![
        (
            "wallet_page.safe",
            vec![
                row("settings.words", "settings.words_about", "common.open", Opens::Tool(Tool::Words)),
                row("settings.backup", "settings.backup_about", "settings.backup_go", Opens::Tool(Tool::Backup)),
                row("settings.restore", "settings.restore_about", "settings.restore_go", Opens::Tool(Tool::Restore)),
            ],
        ),
        (
            "wallet_page.receive",
            vec![
                row("settings.addresses", "settings.addresses_about", "common.open", Opens::Tool(Tool::Addresses)),
                row("sign.row", "sign.row_about", "common.open", Opens::Tool(Tool::Message)),
            ],
        ),
        ("wallet_page.others", vec![row("settings.shared", "settings.shared_about", "common.open", Opens::Tool(Tool::Shared))]),
        ("wallet_page.more", more),
    ]
}

#[derive(Default)]
pub struct WalletPage {
    ledger: Ledger,
    tools: WalletTools,
    locks: Locks,
    in_locks: bool,
}

/// A tool's name and line on the left, its button on the right. True when the button is pressed.
fn tool_row(ui: &mut Ui, cx: &Cx, keys: (&str, &str, &str)) -> bool {
    let t = cx.t;
    let go = cx.say(keys.2);
    let room = ui.available_width() - widgets::button_width(ui, go) - tokens::GAP;
    let pressed = ui
        .horizontal_top(|ui| {
            ui.allocate_ui_with_layout(vec2(room, 0.0), Layout::top_down(Align::Min), |ui| {
                ui.set_width(room);
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.add(Label::new(RichText::new(cx.say(keys.0)).color(t.text)).wrap());
                explain::short(ui, t, cx.say(keys.1));
            });
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| button(ui, t, go, Kind::Plain).clicked()).inner
        })
        .inner;
    ui.add_space(tokens::GAP);
    pressed
}

impl WalletPage {
    fn tools_column(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        for (heading, rows) in groups(cx.expert()) {
            card(ui, t, |ui| {
                section(ui, cx.say(heading));
                ui.add_space(tokens::GAP);
                for row in rows {
                    if tool_row(ui, cx, row.keys) {
                        match row.opens {
                            Opens::Tool(tool) => self.tools.open(tool),
                            Opens::Locks => self.in_locks = true,
                        }
                    }
                }
            });
            ui.add_space(tokens::GAP);
        }
    }

    fn activity(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let still = cx.settings.reduce_motion || desktop_reduces_motion();
        section(ui, cx.say("home.ledger"));
        ui.add_space(2.0);
        if !self.ledger.show(ui, cx, ALL_ROWS, still) {
            meta(ui, cx.say("home.empty_body"), cx.t);
        }
    }
}

impl Page for WalletPage {
    fn title(&self) -> &'static str {
        "page.wallet"
    }

    fn icon(&self) -> Icon {
        Icon::Wallet
    }

    fn daily(&self) -> bool {
        true
    }

    fn show(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        if self.in_locks && cx.expert() {
            if button(ui, t, cx.say("wallet_page.back"), Kind::Quiet).clicked() {
                self.in_locks = false;
            }
            ui.add_space(tokens::GAP);
            self.locks.show(ui, cx);
            return;
        }
        self.in_locks = false;
        widgets::title(ui, cx.say("page.wallet"));
        if ui.available_width() < tokens::COMPACT {
            self.tools_column(ui, cx);
            ui.add_space(tokens::GAP_WIDE);
            self.activity(ui, cx);
        } else {
            let total = ui.available_width();
            let left = total - TOOLS_WIDTH - tokens::GAP_PAGE;
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(vec2(left, 0.0), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(left);
                    self.activity(ui, cx);
                });
                ui.add_space(tokens::GAP_PAGE - ui.spacing().item_spacing.x);
                ui.allocate_ui_with_layout(vec2(TOOLS_WIDTH, 0.0), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(TOOLS_WIDTH);
                    self.tools_column(ui, cx);
                });
            });
        }
        self.tools.show(ui, cx);
    }
}
