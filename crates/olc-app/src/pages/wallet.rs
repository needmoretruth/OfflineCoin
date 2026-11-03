//! The wallet, as Bitcoin Core's Overview, Send, Receive and Transactions in one place: the balance
//! with Receive and Send on top, everything that happened to its coins under it, and its tools
//! grouped by what a person wants to do: keep it safe, receive and prove, share with others, and
//! the rest. Expert mode adds locks. Each tool says in one line what it is for and opens in a sheet.
use super::home::balance_line;
use super::history::HistoryList;
use super::locks::Locks;
use super::money::{ReceiveSheet, SendSheet};
use super::wallet_tools::{Tool, WalletTools};
use super::{Cx, Page};
use crate::core::{Command, Personal};
use crate::ui::explain;
use crate::ui::floating::sheet;
use crate::ui::format;
use crate::ui::theme::desktop_reduces_motion;
use crate::ui::tokens;
use crate::ui::widgets::{self, button, card, meta, section, Kind};
use crate::ui::Icon;
use egui::{vec2, Align, Label, Layout, RichText, Ui};

/// The tools' column beside the rows.
const TOOLS_WIDTH: f32 = 400.0;
/// What a switch at the end of a row takes, with its edge.
const SWITCH_ROOM: f32 = 48.0;
/// A switch is shorter than a button: this lowers its middle to a button's in the rows around it.
const SWITCH_DROP: f32 = 5.0;

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

/// Receive or Send, open over the page.
enum Money {
    Receive(ReceiveSheet),
    Send(SendSheet),
}

#[derive(Default)]
pub struct WalletPage {
    history: HistoryList,
    tools: WalletTools,
    locks: Locks,
    in_locks: bool,
    money: Option<Money>,
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

/// A behaviour's name and line on the left, its switch on the right. The new value when flipped.
fn switch_row(ui: &mut Ui, cx: &Cx, keys: (&str, &str), on: bool) -> Option<bool> {
    let t = cx.t;
    let mut now = on;
    let room = ui.available_width() - SWITCH_ROOM - tokens::GAP;
    let flipped = ui
        .horizontal_top(|ui| {
            ui.allocate_ui_with_layout(vec2(room, 0.0), Layout::top_down(Align::Min), |ui| {
                ui.set_width(room);
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.add(Label::new(RichText::new(cx.say(keys.0)).color(t.text)).wrap());
                explain::short(ui, t, cx.say(keys.1));
            });
            let name = cx.say(keys.0);
            let switch = |ui: &mut Ui| {
                ui.add_space(SWITCH_DROP);
                widgets::switch(ui, t, &mut now, name).clicked()
            };
            ui.with_layout(Layout::top_down(Align::Max), switch).inner
        })
        .inner;
    ui.add_space(tokens::GAP);
    flipped.then_some(now)
}

/// How this wallet pays: merging small coins, and in expert mode signing the old way. They sit
/// beside the history, where a merge shows as its own row.
fn paying(ui: &mut Ui, cx: &Cx) {
    let t = cx.t;
    let node = &cx.snap.node;
    card(ui, t, |ui| {
        section(ui, cx.say("wallet_page.paying"));
        ui.add_space(tokens::GAP);
        if let Some(on) = switch_row(ui, cx, ("storage.merge", "storage.merge_about"), node.merging) {
            cx.send(Command::Own(Personal::SetMerging(on)));
        }
        if cx.expert() {
            if let Some(on) = switch_row(ui, cx, ("pro.version_one", "pro.version_one_about"), node.version_one) {
                cx.send(Command::Own(Personal::SetVersionOne(on)));
            }
        }
    });
    ui.add_space(tokens::GAP);
}

impl WalletPage {
    fn tools_column(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        for (heading, rows) in groups(cx.expert()) {
            if heading == "wallet_page.more" {
                paying(ui, cx);
            }
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

    /// The balance, what is still maturing, and the two things a person does with coins.
    fn balance(&mut self, ui: &mut Ui, cx: &Cx, still: bool) {
        let t = cx.t;
        let node = &cx.snap.node;
        card(ui, t, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    meta(ui, cx.say("home.balance"), t);
                    balance_line(ui, t, node.balance, cx.ticker(), still);
                    // The line keeps its room, so Send and Receive stay put when the first block arrives.
                    widgets::slot(ui, |ui| {
                        ui.set_min_height(tokens::META_LINE);
                        if node.maturing > 0 {
                            meta(ui, &cx.fill("home.soon", &[&format::coins(node.maturing, cx.ticker())]), t);
                        }
                    });
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if button(ui, t, cx.say("home.send"), Kind::Plain).clicked() {
                        self.money = Some(Money::Send(SendSheet::default()));
                    }
                    if button(ui, t, cx.say("home.receive"), Kind::Main).clicked() {
                        self.money = Some(Money::Receive(ReceiveSheet::for_address(&node.address)));
                    }
                });
            });
        });
    }

    fn money_sheet(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        let closed = match &mut self.money {
            None => false,
            Some(Money::Receive(receive)) => {
                let (done, outside) = sheet(ui, t, "wallet-receive", tokens::SLIP_WIDTH, |ui| receive.show(ui, cx));
                done || outside
            }
            Some(Money::Send(send)) => {
                let (done, outside) = sheet(ui, t, "wallet-send", tokens::SLIP_WIDTH, |ui| send.show(ui, cx));
                done || outside
            }
        };
        if closed {
            self.money = None;
        }
    }

    fn activity(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let still = cx.settings.reduce_motion || desktop_reduces_motion();
        self.balance(ui, cx, still);
        ui.add_space(tokens::GAP_WIDE);
        section(ui, cx.say(HistoryList::TITLE));
        ui.add_space(2.0);
        self.history.full(ui, cx);
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

    fn open_inside(&mut self, title: &str) -> bool {
        let locks = title == "page.locks";
        self.in_locks |= locks;
        locks
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
        self.money_sheet(ui, cx);
    }
}
