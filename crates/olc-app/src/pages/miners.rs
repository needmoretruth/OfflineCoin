//! Every miner this computer runs in one table: this app's own first, then each one added, with
//! what it mines at, its part of this computer's limit, whose wallet it pays and a switch. Added
//! miners can be taken out from their row. The table is where miners are managed; each one's
//! memory, disk and payments stay in its own card further down the Mining page.
use super::{Cx, Slipping};
use crate::ui::floating::Slip;
use super::pool_join::standing_words;
use crate::core::{Command, Who};
use crate::local::{Holding, LocalView, NodeSpec, Preset, WalletChoice};
use crate::ui::format;
use crate::ui::tokens::{self, Tokens};
use crate::ui::widgets::{button, cell, dot, header_cell, switch, Columns, Kind};
use egui::{Label, RichText, Ui};

/// Name, speed, limit, wallet, switch, and room for the button that takes a miner out.
const WEIGHTS: [f32; 6] = [1.4, 1.1, 1.1, 1.0, 0.55, 1.05];
const HEADINGS: [&str; 6] = ["local.name", "mining.col_speed", "mining.col_limit", "mining.col_wallet", "mining.col_on", ""];
/// What a new miner may hold, as the Lab's miners do: plenty for a coin made on this computer.
const NEW_MEMORY_MIB: u64 = 64;
const NEW_DISK_MIB: u64 = 64;

/// What one row shows, worked out before it is drawn.
struct Row {
    name: String,
    running: bool,
    speed: String,
    limit: String,
    wallet: &'static str,
    switch: Switch,
    /// The added miner's number, for the button that takes it out.
    remove: Option<u32>,
}

/// What the last control in a row does.
enum Switch {
    /// This app's miner, on or off.
    App(bool),
    /// An added miner that runs, on or off.
    Node(u32, bool),
    /// An added miner that is stopped: it starts first.
    Start(u32),
    /// A miner that keeps only headers cannot mine.
    None,
}

/// A speed in a cell. Plain words need room, so basic mode puts the unit in the heading and the
/// figure alone here; the standard H/s is short enough to stay with each figure.
fn figure(cx: &Cx, rate: u64) -> String {
    if cx.expert() {
        format::rate(cx.voice(), rate)
    } else {
        format::number(rate.into())
    }
}

/// An added miner's part of the limit: what it is held to while it mines, its own speed while it
/// rests if it has one, and otherwise the equal part it would take.
pub fn part_text(cx: &Cx, spec: &NodeSpec) -> String {
    match (cx.snap.budget.part_of(spec.id), spec.hash_cap) {
        (Some(part), _) => figure(cx, part),
        (None, 0) => cx.say("budget.equal_part").to_string(),
        (None, own) => figure(cx, own),
    }
}

fn app_row(cx: &Cx) -> Row {
    let node = &cx.snap.node;
    let budget = &cx.snap.budget;
    let speed = if node.mining { figure(cx, node.hashrate) } else { cx.say("mining.off").to_string() };
    // This app's part waits for it while it rests, so the others keep their speed.
    let limit = if budget.unbounded() { cx.say("mining.no_cap").to_string() } else { figure(cx, budget.own) };
    Row {
        name: cx.say("mining.this_app").to_string(),
        running: node.mining,
        speed,
        limit,
        wallet: cx.say("mining.wallet_here"),
        switch: if node.light.is_some() { Switch::None } else { Switch::App(node.mining) },
        remove: None,
    }
}

fn node_row(cx: &Cx, view: &LocalView) -> Row {
    let id = view.spec.id;
    let mining = view.status.as_ref().filter(|status| status.mining);
    let switch = match () {
        () if view.spec.holding == Holding::Headers => Switch::None,
        () if view.pid.is_none() => Switch::Start(id),
        () => Switch::Node(id, mining.is_some()),
    };
    let wallet = match view.spec.wallet {
        WalletChoice::Own => "local.wallet_own",
        WalletChoice::Shared => "local.wallet_shared",
    };
    Row {
        name: view.spec.name.clone(),
        running: view.pid.is_some(),
        speed: mining.map_or_else(|| cx.say("mining.off").to_string(), |status| figure(cx, status.hashrate)),
        limit: part_text(cx, &view.spec),
        wallet: cx.say(wallet),
        switch,
        remove: Some(id),
    }
}

fn remove_slip(cx: &Cx, name: &str, id: u32) -> Slip {
    cx.slip_for(Slipping {
        what: cx.say("local.remove"),
        rows: vec![(cx.say("local.name"), name.to_string())],
        warning: cx.say("local.remove_warning"),
        sign: cx.say("local.remove"),
        danger: true,
        order: vec![Command::RemoveNode(id)],
    })
}

/// The switch or button at the end of a row.
fn control(ui: &mut Ui, cx: &Cx, t: &Tokens, row: &Row) {
    match row.switch {
        Switch::App(on) => {
            let mut on = on;
            if switch(ui, t, &mut on, &row.name).changed() {
                cx.send(Command::SetMining(on));
            }
        }
        Switch::Node(id, on) => {
            let mut on = on;
            if switch(ui, t, &mut on, &row.name).changed() {
                cx.send(Command::NodeMining { id, on });
            }
        }
        Switch::Start(id) => {
            if button(ui, t, cx.say("local.start"), Kind::Quiet).clicked() {
                cx.send(Command::StartNode(id));
            }
        }
        Switch::None => {
            ui.label(RichText::new("-").color(t.muted));
        }
    }
}

/// The table. A press on a row's remove button asks first, as every removal does.
pub fn table(ui: &mut Ui, cx: &mut Cx) {
    let t = *cx.t;
    let mut rows = vec![app_row(cx)];
    rows.extend(cx.snap.local.iter().map(|view| node_row(cx, view)));
    Columns::new(ui, &WEIGHTS).row(ui, |ui, index| {
        if !HEADINGS[index].is_empty() {
            header_cell(ui, &t, cx.say(HEADINGS[index]));
        }
    });
    // Every row holds a switch, and most a button, so each row centres its words on the button.
    let columns = Columns::new(ui, &WEIGHTS).with_controls();
    let mut slip = None;
    for row in &rows {
        columns.row(ui, |ui, index| match index {
            0 => {
                dot(ui, if row.running { t.good } else { t.muted });
                ui.add(Label::new(row.name.as_str()).truncate());
            }
            1 => cell(ui, RichText::new(&row.speed).color(if row.running { t.text } else { t.muted })),
            2 => cell(ui, RichText::new(&row.limit).color(t.soft)),
            3 => cell(ui, RichText::new(row.wallet).color(t.soft)),
            4 => control(ui, cx, &t, row),
            _ => {
                if let Some(id) = row.remove {
                    if button(ui, &t, cx.say("local.remove"), Kind::Quiet).clicked() {
                        slip = Some(remove_slip(cx, &row.name, id));
                    }
                }
            }
        });
    }
    if slip.is_some() {
        *cx.slip = slip;
    }
    in_pools(ui, cx, &t);
}

/// A line for each miner in the table that joined a pool: its speed above is pool work, and its
/// coins come from the pool. The line stays while the miner is joined, whatever the link does, so
/// the page holds still.
fn in_pools(ui: &mut Ui, cx: &Cx, t: &Tokens) {
    for link in &cx.snap.pool.links {
        let name = match link.join.who {
            Who::Own => cx.say("mining.this_app").to_string(),
            Who::Node(id) => match cx.snap.local.iter().find(|view| view.spec.id == id) {
                Some(view) => view.spec.name.clone(),
                None => continue,
            },
            // Boards say it on their own lines.
            Who::Board(_) | Who::Lent(_) => continue,
        };
        let (colour, said) = standing_words(cx, link);
        ui.horizontal(|ui| {
            dot(ui, colour);
            ui.label(RichText::new(format!("{name}: {said}")).size(tokens::META).color(t.soft));
        });
    }
}

/// What one more miner would start at, said before it is added.
pub fn next_line(ui: &mut Ui, cx: &Cx) {
    let next = cx.snap.budget.next;
    if next > 0 {
        let line = cx.fill("budget.next", &[&format::rate(cx.voice(), next)]);
        ui.add(Label::new(RichText::new(line).size(tokens::META).color(cx.t.soft)).wrap());
    }
}

/// One more miner on this computer with its own wallet, mining at once on an equal part of this
/// computer's limit, never above it. Every number stays editable in its card. `next` is the first
/// id not yet asked for, so two clicks before the core answers still make two miners.
pub fn add(cx: &Cx, next: &mut u32) {
    let known = cx.snap.local.iter().map(|view| view.spec.id).max().map_or(0, |id| id + 1);
    let id = known.max(*next);
    *next = id + 1;
    let spec = NodeSpec {
        name: cx.fill("mining.miner_name", &[&(id + 1).to_string()]),
        hash_cap: 0,
        memory_mib: NEW_MEMORY_MIB,
        disk_mib: NEW_DISK_MIB,
        wallet: WalletChoice::Own,
        joined: true,
        mining: true,
        ..Preset::Custom.spec(id)
    };
    cx.send(Command::AddNode(Box::new(spec)));
}
