//! What each participant is called and the few facts it says: one short line under its token,
//! and at most three lines in its panel. Basic mode counts in plain words; expert mode says
//! heights, hash rates and the mempool.
use crate::i18n::{fill, text, Voice};
use crate::local::lab_net::{Kind, Member, NetState, Trick};
use crate::settings::Mode;
use crate::ui::tokens::Tokens;
use egui::Color32;

pub fn kind_key(kind: Kind) -> &'static str {
    match kind {
        Kind::Miner => "net.kind_miner",
        Kind::Pool => "net.kind_pool",
        Kind::Full => "net.kind_full",
        Kind::Pruned => "net.kind_pruned",
        Kind::Light => "net.kind_light",
        Kind::Attacker => "net.kind_attacker",
        Kind::Exploiter => "net.kind_exploiter",
        Kind::Spammer => "net.kind_spammer",
        Kind::Sender => "net.kind_sender",
        Kind::Receiver => "net.kind_receiver",
    }
}

/// Its kind and its number among its kind: "Miner 2".
pub fn name(voice: Voice, member: &Member) -> String {
    format!("{} {}", text(voice, kind_key(member.kind)), member.number)
}

fn lab(member: &Member) -> crate::local::lab_side::LabLine {
    member.status.as_ref().and_then(|status| status.lab.clone()).unwrap_or_default()
}

/// An amount of the practice coin as every page says amounts: rounded for a plain reader, to the
/// last unit for an expert, with the coin's ticker.
pub(super) fn coins(voice: Voice, units: u64) -> String {
    crate::ui::format::payout(voice, units, crate::local::lab::LAB_TICKER)
}

/// The one line under a token, and its colour.
pub fn status_line(t: &Tokens, voice: Voice, member: &Member) -> (String, Color32) {
    if member.ended.is_some() {
        return (text(voice, "net.ended").to_string(), t.bad);
    }
    if member.paused {
        let key = if member.kind.mines() || member.kind.speed().is_some() { "net.paused" } else { "net.cut_off" };
        return (text(voice, key).to_string(), t.muted);
    }
    if member.busy && member.kind == Kind::Attacker {
        return (text(voice, "net.attacking").to_string(), t.bad);
    }
    let Some(status) = &member.status else {
        return (text(voice, "net.wait_start").to_string(), t.muted);
    };
    let line = lab(member);
    let expert = voice.mode == Mode::Expert;
    let say = |key: &str, value: u64| fill(text(voice, key), &[&value.to_string()]);
    let shown = match member.kind {
        // In a pool the pool finds the blocks: a member counts the shares it handed in.
        Kind::Miner if member.pool.is_some() => say("net.shares", line.accepted),
        Kind::Miner | Kind::Attacker if expert => format!("{} H/s · {}", status.hashrate, say("net.found", status.blocks_mined)),
        Kind::Miner | Kind::Attacker => say("net.found", status.blocks_mined),
        Kind::Pool => say("net.members", line.workers.len() as u64),
        Kind::Full => say("net.holds_all", status.height),
        Kind::Pruned if expert => say("net.holds_recent", line.oldest_held),
        Kind::Pruned => say("net.holds_recent", status.height.saturating_sub(line.oldest_held) + 1),
        Kind::Light => say("net.holds_summary", status.headers.unwrap_or(status.height)),
        Kind::Receiver => say("net.received", member.received.payments),
        Kind::Spammer => say("net.spam_sent", line.spam_sent),
        Kind::Sender | Kind::Exploiter => match member.waits {
            Some(key) => text(voice, key).to_string(),
            None if member.kind == Kind::Exploiter => say("net.refused_count", member.tricks.iter().filter(|row| row.refused).count() as u64),
            None => say("net.sent", member.received.sent),
        },
    };
    let colour = if member.kind == Kind::Spammer && line.waiting_bytes > line.max_block_bytes { t.warn } else { t.muted };
    (shown, colour)
}

/// Each trick the bug exploiter tried: what it did, on how many nodes and how many refused it,
/// and the rule that stopped it.
pub fn tricks(voice: Voice, member: &Member, state: &NetState) -> Vec<(String, String)> {
    let mut lines = Vec::new();
    for trick in Trick::ALL {
        let rows: Vec<_> = member.tricks.iter().filter(|row| row.trick == trick).collect();
        if rows.is_empty() {
            continue;
        }
        let refused = rows.iter().filter(|row| row.refused).count();
        let counted = fill(text(voice, "net.fact_trick"), &[&rows.len().to_string(), &refused.to_string()]);
        lines.push((text(voice, trick.key()).to_string(), format!("{counted} {}", text(voice, trick.rule_key()))));
    }
    if !lines.is_empty() && state.members.iter().any(|other| other.kind == Kind::Light) {
        lines.push((String::new(), text(voice, "net.trick_skip_light").to_string()));
    }
    lines
}

/// At most three lines that say what it holds and does now.
pub fn facts(voice: Voice, member: &Member, state: &NetState) -> Vec<String> {
    let Some(status) = &member.status else {
        return vec![text(voice, "net.wait_start").to_string()];
    };
    let line = lab(member);
    let with = |key: &str, values: &[&str]| fill(text(voice, key), values);
    let mut facts = Vec::new();
    match member.kind {
        Kind::Miner => match member.pool.and_then(|pool| state.members.iter().find(|other| other.id == pool)) {
            Some(pool) => {
                let paid = member.received.from_pool;
                facts.push(with("net.fact_in_pool", &[&name(voice, pool), &coins(voice, paid)]));
            }
            None => facts.push(with("net.fact_miner", &[&status.blocks_mined.to_string(), &coins(voice, status.balance)])),
        },
        Kind::Attacker => {
            let total: u64 = state.members.iter().filter_map(|other| other.status.as_ref()).map(|other| other.hashrate).sum();
            let share = (status.hashrate * 100).checked_div(total).unwrap_or(0);
            facts.push(with("net.fact_attacker", &[&share.to_string(), &coins(voice, status.balance)]));
        }
        Kind::Pool => {
            facts.push(with("net.fact_pool", &[&line.workers.len().to_string(), &line.pool_found.to_string()]));
            for worker in line.workers.iter().take(2) {
                let who = state
                    .members
                    .iter()
                    .find(|other| other.address() == Some(worker.address.as_str()))
                    .map_or_else(|| worker.address.chars().take(10).collect(), |other| name(voice, other));
                facts.push(with("net.fact_worker", &[&who, &worker.accepted.to_string(), &coins(voice, worker.paid)]));
            }
        }
        Kind::Full => facts.push(with("net.fact_full", &[&status.height.to_string()])),
        Kind::Pruned => facts.push(with("net.fact_pruned", &[&line.oldest_held.to_string()])),
        Kind::Light => facts.push(with("net.fact_light", &[&status.headers.unwrap_or(status.height).to_string()])),
        Kind::Sender => facts.push(with("net.fact_sender", &[&member.received.sent.to_string(), &coins(voice, status.balance)])),
        Kind::Receiver => {
            let got = member.received;
            facts.push(with("net.fact_receiver", &[&got.payments.to_string(), &got.settled.to_string(), &got.undone.to_string()]));
        }
        Kind::Spammer => facts.push(with("net.fact_spammer", &[&line.spam_sent.to_string(), &(line.waiting_bytes / 1024).to_string()])),
        Kind::Exploiter => {
            let rows = &member.tricks;
            if let Some(newest) = rows.last() {
                let tried = Trick::ALL.iter().filter(|trick| rows.iter().any(|row| row.trick == **trick)).count();
                let nodes: std::collections::BTreeSet<u32> = rows.iter().map(|row| row.by).collect();
                let refused = rows.iter().filter(|row| row.refused).count();
                facts.push(with("net.fact_tricks", &[&tried.to_string(), &nodes.len().to_string(), &refused.to_string()]));
                facts.push(text(voice, newest.trick.key()).to_string());
                let through = rows.iter().filter(|row| !row.refused && row.failed.is_none()).count();
                let failed = rows.iter().filter(|row| row.failed.is_some()).count();
                facts.push(match (through, failed) {
                    (1.., _) => with("net.trick_let_through", &[&through.to_string()]),
                    (0, 1..) => with("net.trick_failed", &[&failed.to_string()]),
                    _ => text(voice, newest.trick.rule_key()).to_string(),
                });
            }
        }
    }
    if let Some(key) = member.waits {
        facts.insert(0, text(voice, key).to_string());
    }
    facts.truncate(3);
    facts
}

