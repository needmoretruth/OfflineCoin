//! The parts of the pool page that show the pool at work: each worker's numbers, the blocks the
//! pool found and where every coin of each went, and what is behind Details.
use super::pool::label_of;
use super::Cx;
use crate::core::{BlockState, FoundRow, PoolView, SwitchedOff, Who, WorkerRow};
use crate::ui::explain::{self, Choice};
use crate::ui::format;
use crate::ui::tokens::{self, Tokens};
use crate::ui::widgets::{cell, dot, header_cell, meta, right_cell, tag, Columns};
use egui::{vec2, Align, Color32, CornerRadius, Layout, Rect, RichText, Sense, Ui};

/// The blocks listed before older ones fold away.
const BLOCKS_SHOWN: usize = 6;
const BAR: f32 = 18.0;
/// The first column of the workers table that holds a figure: accepted, refused, paid, difficulty.
const FIRST_FIGURE: usize = 3;

/// The part after the address in `address.worker`.
fn worker_of(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(_, worker)| worker)
}

fn name_of(cx: &Cx, row: &WorkerRow) -> String {
    match row.here {
        Some(who) => label_of(cx, Some(who), &row.worker),
        None => row.worker.clone(),
    }
}

/// Each worker: speed, its last share, what was taken and refused, and what it was paid.
pub fn workers_table(ui: &mut Ui, cx: &Cx, view: &PoolView) {
    let t = cx.t;
    let unlisted = view.switched_off.iter().filter(|off| !view.workers.iter().any(|row| row.name == off.user));
    for off in unlisted {
        switched_off(ui, cx, off, Some(worker_of(&off.user)));
    }
    if view.workers.is_empty() {
        meta(ui, cx.say("pool.no_workers_yet"), t);
        return;
    }
    let mut keys = vec!["pool.who", "pool.speed", "pool.last_share", "pool.accepted", "pool.refused", "pool.paid"];
    let mut weights = vec![2.4, 1.4, 1.1, 0.8, 0.8, 1.4];
    if cx.expert() {
        keys.push("pro.difficulty");
        weights.push(0.9);
    }
    let columns = Columns::new(ui, &weights);
    // Figures stand right in their columns, and so do the words above them.
    columns.row(ui, |ui, index| {
        if index >= FIRST_FIGURE {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| header_cell(ui, t, cx.say(keys[index])));
        } else {
            header_cell(ui, t, cx.say(keys[index]));
        }
    });
    for row in &view.workers {
        columns.row(ui, |ui, index| match index {
            0 => {
                dot(ui, if row.online { t.good } else { t.muted });
                let name = ui.add(egui::Label::new(name_of(cx, row)).truncate());
                name.on_hover_text(&row.name);
                if row.here.is_none() {
                    tag(ui, cx.say("pool.elsewhere"), t.info, t);
                }
            }
            1 => cell(ui, format::rate(cx.voice(), row.speed)),
            2 => cell(ui, last_share(cx, row.last_share_secs)),
            3 => right_cell(ui, format::number(row.accepted.into())),
            4 => right_cell(ui, format::number(row.refused.into())),
            5 => right_cell(ui, format::payout(cx.voice(), row.paid, cx.ticker())),
            _ => right_cell(ui, format::number(row.difficulty.into())),
        });
        // A board the pool switched off for never taking its work says why under its row.
        if let Some(off) = view.switched_off.iter().find(|off| off.user == row.name) {
            switched_off(ui, cx, off, None);
        }
    }
}

/// Why the pool switched a board off and what to do, with its name when no row above names it.
fn switched_off(ui: &mut Ui, cx: &Cx, off: &SwitchedOff, name: Option<&str>) {
    let why = cx.fill(off.why, &[&off.relay]);
    let line = match name {
        Some(name) => format!("{name} · {why}"),
        None => why,
    };
    ui.horizontal(|ui| {
        ui.add_space(tokens::GAP_WIDE);
        ui.label(RichText::new(line).size(tokens::META).color(cx.t.warn));
    });
}

fn last_share(cx: &Cx, secs: Option<u64>) -> String {
    match secs {
        Some(secs) => format::ago(cx.voice(), secs.saturating_mul(1_000), 0),
        None => cx.say("pool.never").to_string(),
    }
}

fn state_of(t: &Tokens, state: BlockState) -> (&'static str, Color32) {
    match state {
        BlockState::Sent => ("pool.state_sent", t.soft),
        BlockState::Waiting => ("pool.state_waiting", t.info),
        BlockState::Paid => ("pool.state_paid", t.good),
        BlockState::Lost => ("pool.state_lost", t.bad),
    }
}

/// The blocks the pool found, newest first. The newest one shared out shows where its coins went.
pub fn found_blocks(ui: &mut Ui, cx: &Cx, view: &PoolView) {
    let t = cx.t;
    if view.found.is_empty() {
        meta(ui, cx.say("pool.found_none"), t);
        return;
    }
    let newest_paid = view.found.iter().position(|row| row.state == BlockState::Paid);
    for (at, row) in view.found.iter().take(BLOCKS_SHOWN).enumerate() {
        ui.horizontal(|ui| {
            ui.label(RichText::new(cx.fill("pool.block", &[&format::number(row.height.into())])).strong());
            ui.label(format::payout(cx.voice(), row.value, cx.ticker()));
            let (key, colour) = state_of(t, row.state);
            tag(ui, cx.say(key), colour, t);
            meta(ui, &format::ago(cx.voice(), row.secs_ago.saturating_mul(1_000), 0), t);
        });
        if row.state != BlockState::Paid {
            continue;
        }
        if Some(at) == newest_paid {
            split(ui, cx, view, row);
        } else {
            let labels = (cx.say("pool.show_split"), cx.say("explain.hide"));
            explain::details(ui, t, ("pool-split", row.height), labels, |ui| split(ui, cx, view, row));
        }
        ui.add_space(tokens::GAP);
    }
}

/// One piece of a bar: whose, how much, and how it is drawn.
struct Part {
    name: String,
    units: u64,
    colour: Color32,
    /// The pool's own share: a thin line in the bar, since it is nobody's work.
    thin: bool,
}

/// Colour is for the person and for states only: this computer's part is the person's blue, and
/// every other worker a grey, from the strongest ink to a faint one in the order they are listed.
fn shades(t: &Tokens, mine: &[bool]) -> Vec<Color32> {
    let others = mine.iter().filter(|mine| !**mine).count();
    let faint = t.soft.lerp_to_gamma(t.card_top, 0.55);
    let mut step = 0;
    let mut grey = || {
        let at = if others > 1 { step as f32 / (others - 1) as f32 } else { 0.0 };
        step += 1;
        t.soft.lerp_to_gamma(faint, at)
    };
    mine.iter().map(|mine| if *mine { t.info } else { grey() }).collect()
}

/// Where a block's coins went, as one bar cut by worker, then each part in words.
fn split(ui: &mut Ui, cx: &Cx, view: &PoolView, row: &FoundRow) {
    let t = cx.t;
    let here = |name: &str| view.workers.iter().find(|worker| worker.name == name).and_then(|worker| worker.here);
    let mine: Vec<bool> = row.parts.iter().map(|(name, _)| here(name) == Some(Who::Own)).collect();
    let mut parts: Vec<Part> = row
        .parts
        .iter()
        .zip(shades(t, &mine))
        .map(|((name, amount), colour)| {
            let who = here(name);
            let shown = if who.is_some() { label_of(cx, who, worker_of(name)) } else { worker_of(name).to_string() };
            Part { name: shown, units: *amount, colour, thin: false }
        })
        .collect();
    if row.pool_fee > 0 {
        parts.push(Part { name: cx.say("pool.kept").to_string(), units: row.pool_fee, colour: t.muted, thin: true });
    }
    ui.add_space(4.0);
    bar(ui, t, &parts, row.value, |units| format::payout_amount(cx.voice(), units));
    ui.horizontal_wrapped(|ui| {
        for part in &parts {
            swatch(ui, part.colour);
            ui.label(format!("{} {}", part.name, format::payout(cx.voice(), part.units, cx.ticker())));
            ui.add_space(tokens::GAP);
        }
    });
    let paid: u64 = parts.iter().map(|part| part.units).sum();
    let left = row.value.saturating_sub(paid);
    let shown = |units| format::payout(cx.voice(), units, cx.ticker());
    let sum = cx.fill("pool.split_sum", &[&shown(paid), &shown(row.value)]);
    meta(ui, &sum, t);
    if left > 0 {
        meta(ui, &cx.fill("pool.payment_fee", &[&shown(left)]), t);
    }
}

fn swatch(ui: &mut Ui, colour: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(2), colour);
}

/// One bar, cut in proportion to each part, read aloud as each part's name and `say` of it.
fn bar(ui: &mut Ui, t: &Tokens, parts: &[Part], whole: u64, say: impl Fn(u64) -> String) {
    let width = ui.available_width().min(720.0);
    let (rect, response) = ui.allocate_exact_size(vec2(width, BAR), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(tokens::RADIUS_TAG), t.line);
    let whole = whole.max(1) as f32;
    let mut left = rect.left();
    for part in parts {
        let wide = rect.width() * (part.units as f32 / whole);
        let piece = Rect::from_min_size(egui::pos2(left, rect.top()), vec2(wide, rect.height()));
        let piece = if part.thin { piece.shrink2(vec2(0.5, rect.height() * 0.3)) } else { piece.shrink2(vec2(0.5, 0.0)) };
        painter.rect_filled(piece, CornerRadius::same(3), part.colour);
        left += wide;
    }
    let said: Vec<String> = parts.iter().map(|part| format!("{} {}", part.name, say(part.units))).collect();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, said.join(", ")));
}

/// Behind Details: a picture of one block cut by work, an analogy, then the three ways to be paid
/// on the same four questions.
pub fn how_it_works(ui: &mut Ui, cx: &Cx, last_shares: u64) {
    let t = cx.t;
    // Three miners, none of them this computer.
    let example: Vec<Part> = [50, 30, 20]
        .iter()
        .zip(shades(t, &[false; 3]))
        .enumerate()
        .map(|(index, (part, colour))| Part { name: cx.fill("pool.pic_miner", &[&(index + 1).to_string()]), units: *part, colour, thin: false })
        .collect();
    meta(ui, cx.say("pool.pic_work"), t);
    bar(ui, t, &example, 100, |part| format!("{part}%"));
    ui.add_space(4.0);
    meta(ui, cx.say("pool.pic_coins"), t);
    bar(ui, t, &example, 100, |part| format!("{part}%"));
    ui.horizontal_wrapped(|ui| {
        for part in &example {
            swatch(ui, part.colour);
            ui.label(format!("{} {}%", part.name, part.units));
            ui.add_space(tokens::GAP);
        }
    });
    ui.add_space(tokens::GAP);
    explain::analogy(ui, t, cx.say("pool.analogy"));
    ui.add_space(tokens::GAP);
    let counts = cx.fill("pool.v_pplns_counts", &[&format::number(last_shares.into())]);
    let prop = [cx.say("pool.v_each_block"), cx.say("pool.v_small"), cx.say("pool.v_prop_counts"), cx.say("pool.v_prop_hop")];
    let pplns = [cx.say("pool.v_each_block"), cx.say("pool.v_small"), counts.as_str(), cx.say("pool.v_pplns_hop")];
    let solo = [cx.say("pool.v_solo_when"), cx.say("pool.v_large"), cx.say("pool.v_solo_counts"), cx.say("pool.v_solo_hop")];
    let choices = [
        Choice { name: cx.say("pool.m_prop"), values: &prop },
        Choice { name: cx.say("pool.m_pplns"), values: &pplns },
        Choice { name: cx.say("pool.m_solo"), values: &solo },
    ];
    let criteria = [cx.say("pool.c_when"), cx.say("pool.c_swing"), cx.say("pool.c_counts"), cx.say("pool.c_hop")];
    explain::compare(ui, t, "pool-ways", &criteria, &choices);
}

#[cfg(test)]
mod tests {
    use super::worker_of;

    #[test]
    fn a_worker_is_named_by_the_part_after_its_address() {
        assert_eq!(worker_of("olc1abc.laptop"), "laptop");
        assert_eq!(worker_of("alone"), "alone");
    }
}
