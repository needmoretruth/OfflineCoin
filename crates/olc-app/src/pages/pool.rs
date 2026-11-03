//! The pool page: run a pool on this computer that other computers join over the network, the
//! way Bitcoin's pools work, and join one that runs elsewhere. Every miner this app knows is
//! listed, and one that cannot take pool work says why in one line.
use super::pool_parts::{found_blocks, how_it_works, workers_table};
use super::Cx;
use crate::core::{Choice, Command, Method, PoolOrder, PoolSettings, PoolView, Who, LAST_SHARES, MOST_FEE_BPS, POOL_PORT};
use crate::ui::explain;
use crate::ui::format;
use crate::ui::tokens;
use crate::ui::widgets::{self, button, button_on, card, dot, field, meta, number_field, section, segments, switch_row, Kind};
use egui::{RichText, Ui};

/// The narrowest a field for a number is drawn.
const NUMBER: f32 = 110.0;

#[derive(Default)]
pub struct PoolTab {
    /// The choices being made, before Start. Taken from the last pool's once.
    form: Option<PoolSettings>,
    /// The fee as typed, in percent.
    fee: String,
    /// Why Start was not pressed: a key.
    wrong: Option<&'static str>,
}

/// What a miner here is called on this page.
pub fn label_of(cx: &Cx, who: Option<Who>, label: &str) -> String {
    match who {
        Some(Who::Own) => cx.say("pool.this_computer").to_string(),
        _ => label.to_string(),
    }
}

/// A fee in hundredths of a percent, as a person writes it: `1.5`.
pub fn percent(bps: u32) -> String {
    match bps % 100 {
        0 => (bps / 100).to_string(),
        rest if rest % 10 == 0 => format!("{}.{}", bps / 100, rest / 10),
        rest => format!("{}.{rest:02}", bps / 100),
    }
}

/// Hundredths of a percent from what a person typed, or None past the pool's most.
fn fee_of(text: &str) -> Option<u32> {
    let text = text.trim().trim_end_matches('%').trim();
    if text.is_empty() {
        return Some(0);
    }
    let percent: f64 = text.parse().ok()?;
    let bps = (percent * 100.0).round();
    (0.0..=f64::from(MOST_FEE_BPS)).contains(&bps).then_some(bps as u32)
}

impl PoolTab {
    pub fn show(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let view = cx.snap.pool.clone();
        if view.running {
            running(ui, cx, &view);
        } else if view.links.iter().any(|link| !link.here) {
            // A computer that works in a pool elsewhere sees that pool first, then how to open one.
            super::pool_join::page_card(ui, cx, &view);
            ui.add_space(tokens::GAP_WIDE);
            self.form(ui, cx, &view);
            return;
        } else {
            self.form(ui, cx, &view);
        }
        ui.add_space(tokens::GAP_WIDE);
        super::pool_join::page_card(ui, cx, &view);
    }

    fn form(&mut self, ui: &mut Ui, cx: &mut Cx, view: &PoolView) {
        let t = cx.t;
        let form = self.form.get_or_insert_with(|| {
            let mut kept = cx.settings.pool.clone();
            if kept.workers.is_empty() {
                kept.workers = vec![Who::Own];
            }
            kept
        });
        if self.fee.is_empty() {
            self.fee = percent(form.fee_bps);
        }
        card(ui, t, |ui| {
            section(ui, cx.say("pool.run_here"));
            explain::short(ui, t, cx.say("pool.about"));
            explain::details(ui, t, "pool-how", (cx.say("explain.details"), cx.say("explain.hide")), |ui| {
                how_it_works(ui, cx, form.last_shares);
            });
            ui.add_space(tokens::GAP);
            section(ui, cx.say("pool.workers"));
            widgets::slot(ui, |ui| {
                for choice in &view.choices {
                    choice_row(ui, cx, choice, &mut form.workers);
                }
            });
            ui.add_space(tokens::GAP);
            switch_row(ui, t, &mut form.open, cx.say("pool.open"));
            meta(ui, cx.say("pool.open_about"), t);
            ui.add_space(tokens::GAP);
            ui.horizontal(|ui| {
                ui.label(cx.say("pool.port"));
                let mut port = u64::from(form.port);
                if number_field(ui, &mut port, cx.say("pool.port"), NUMBER) {
                    form.port = u16::try_from(port).unwrap_or(POOL_PORT);
                }
                // Said only when the port is not the usual one: beside 3333 it would repeat it.
                if form.port != POOL_PORT {
                    meta(ui, &cx.fill("pool.port_about", &[&POOL_PORT.to_string()]), t);
                }
            });
            if cx.expert() {
                expert_choices(ui, cx, form, &mut self.fee);
            }
            ui.add_space(tokens::GAP);
            let ready = !form.workers.is_empty() || form.open;
            ui.horizontal(|ui| {
                if button_on(ui, t, cx.say("pool.start"), Kind::Main, ready).clicked() {
                    self.wrong = None;
                    match fee_of(&self.fee) {
                        Some(fee) if form.port > 0 => {
                            let mut chosen = form.clone();
                            chosen.fee_bps = if cx.expert() { fee } else { form.fee_bps };
                            cx.send(Command::Pool(PoolOrder::Start(chosen)));
                        }
                        Some(_) => self.wrong = Some("pool.error_port"),
                        None => self.wrong = Some("pro.pool_fee_about"),
                    }
                }
                if !ready {
                    meta(ui, cx.say("pool.choose_one"), t);
                }
            });
            widgets::slot(ui, |ui| {
                let error = self.wrong.or(view.error.as_ref().map(|(key, _)| *key));
                if let Some(key) = error {
                    ui.colored_label(t.bad, cx.say(key));
                }
            });
        });
    }
}

/// One miner that could work: a switch when it can, its name and the one reason when it cannot.
fn choice_row(ui: &mut Ui, cx: &Cx, choice: &Choice, chosen: &mut Vec<Who>) {
    let t = cx.t;
    let name = label_of(cx, choice.who, &choice.label);
    ui.horizontal_wrapped(|ui| {
        match (choice.who, choice.why_not) {
            (Some(who), None) => {
                let mut on = chosen.contains(&who);
                if switch_row(ui, t, &mut on, &name) {
                    chosen.retain(|known| *known != who);
                    if on {
                        chosen.push(who);
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
            ui.add_space(44.0);
            ui.label(RichText::new(cx.say(why)).size(tokens::META).color(t.warn));
        });
    }
}

/// What only an expert sets: how coins are shared, the pool's fee, one share difficulty for all.
fn expert_choices(ui: &mut Ui, cx: &Cx, form: &mut PoolSettings, fee: &mut String) {
    let t = cx.t;
    ui.add_space(tokens::GAP);
    ui.horizontal(|ui| {
        ui.label(cx.say("pro.pool_method"));
        let items = [
            (Method::Proportional, cx.say("pool.m_prop").to_string()),
            (Method::LastShares, cx.say("pool.m_pplns").to_string()),
        ];
        if let Some(method) = segments(ui, t, &items, Some(form.method)) {
            form.method = method;
        }
        if form.method == Method::LastShares {
            ui.label(cx.say("pro.pool_last_n"));
            if form.last_shares == 0 {
                form.last_shares = LAST_SHARES;
            }
            number_field(ui, &mut form.last_shares, cx.say("pro.pool_last_n"), NUMBER);
        }
    });
    ui.horizontal(|ui| {
        ui.label(cx.say("pro.pool_fee"));
        field(ui, fee, "0", NUMBER);
        meta(ui, cx.say("pro.pool_fee_about"), t);
    });
    ui.horizontal(|ui| {
        ui.label(cx.say("pro.share_difficulty"));
        number_field(ui, &mut form.fixed_difficulty, cx.say("pro.share_difficulty"), NUMBER);
        meta(ui, cx.say("pro.pool_vardiff_about"), t);
    });
}

/// The pool as it runs: where others join, who works, the blocks it found and how they were shared.
fn running(ui: &mut Ui, cx: &mut Cx, view: &PoolView) {
    let t = cx.t;
    card(ui, t, |ui| {
        ui.horizontal(|ui| {
            dot(ui, t.good);
            section(ui, cx.say("pool.running"));
        });
        let reach = view.reach.first().cloned().unwrap_or_default();
        let key = if view.open { "pool.join_at" } else { "pool.only_here" };
        ui.horizontal_wrapped(|ui| {
            ui.label(cx.say(key));
            ui.label(RichText::new(&reach).monospace().strong().color(t.text));
            if button(ui, t, cx.say("pool.copy"), Kind::Quiet).clicked() {
                ui.ctx().copy_text(reach.clone());
            }
        });
        if cx.expert() {
            let method = match view.method {
                Method::Proportional => cx.say("pool.m_prop").to_string(),
                Method::LastShares => format!("{} N={}", cx.say("pool.m_pplns"), view.last_shares),
            };
            let difficulty = match view.fixed_difficulty {
                0 => cx.say("pro.pool_vardiff").to_string(),
                fixed => format::number(fixed.into()),
            };
            let plan = cx.fill("pro.pool_plan", &[&method, &percent(view.fee_bps), &difficulty]);
            meta(ui, &plan, t);
        }
        ui.add_space(tokens::GAP);
        totals(ui, cx, view);
        ui.add_space(tokens::GAP);
        if button(ui, t, cx.say("pool.stop"), Kind::Plain).clicked() {
            cx.send(Command::Pool(PoolOrder::Stop));
        }
    });
    ui.add_space(tokens::GAP_WIDE);
    card(ui, t, |ui| {
        section(ui, cx.say("pool.workers"));
        workers_table(ui, cx, view);
    });
    ui.add_space(tokens::GAP_WIDE);
    card(ui, t, |ui| {
        section(ui, cx.say("pool.found_title"));
        found_blocks(ui, cx, view);
    });
}

/// Four numbers a person checks first.
fn totals(ui: &mut Ui, cx: &Cx, view: &PoolView) {
    let t = cx.t;
    let working = view.workers.iter().filter(|row| row.online).count();
    let speed: u64 = view.workers.iter().filter(|row| row.online).map(|row| row.speed).sum();
    let (coins, blocks) = view.found_total();
    let items = [
        ("pool.working", format::number(working as u128)),
        ("pool.speed", format::rate(cx.voice(), speed)),
        ("pool.blocks", format::number(blocks as u128)),
        ("pool.coins_found", format::coins(coins, cx.ticker())),
    ];
    ui.horizontal_wrapped(|ui| {
        for (key, value) in items {
            ui.vertical(|ui| {
                meta(ui, cx.say(key), t);
                ui.label(RichText::new(value).size(tokens::SECTION).strong().color(t.text));
            });
            ui.add_space(tokens::GAP_PAGE);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fee_reads_as_a_person_writes_it() {
        assert_eq!(fee_of("1.5"), Some(150));
        assert_eq!(fee_of(" 2 % "), Some(200));
        assert_eq!(fee_of(""), Some(0));
        assert_eq!(fee_of("5.01"), None);
        assert_eq!(fee_of("-1"), None);
        assert_eq!(percent(150), "1.5");
        assert_eq!(percent(125), "1.25");
        assert_eq!(percent(0), "0");
    }
}
