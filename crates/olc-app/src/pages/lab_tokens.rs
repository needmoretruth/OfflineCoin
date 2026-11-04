//! What each participant is called and the few facts it says: one short line under its token,
//! and at most three lines in its panel. Basic mode counts in plain words; expert mode says
//! heights, hash rates and the mempool.
use crate::core::format_coins;
use crate::i18n::{fill, text, Voice};
use crate::local::lab_net::{Kind, Member, NetState, What};
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

fn coins(units: u64) -> String {
    let shown = format_coins(units);
    match shown.split_once('.') {
        Some((whole, part)) => {
            let part = part.trim_end_matches('0');
            if part.is_empty() { whole.to_string() } else { format!("{whole}.{}", &part[..part.len().min(2)]) }
        }
        None => shown,
    }
}

/// The one line under a token, and its colour.
pub fn status_line(t: &Tokens, voice: Voice, member: &Member) -> (String, Color32) {
    if member.ended.is_some() {
        return (text(voice, "net.wait_start").to_string(), t.bad);
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
        Kind::Miner | Kind::Attacker if expert => format!("{} H/s · {}", status.hashrate, say("net.found", status.blocks_mined)),
        Kind::Miner | Kind::Attacker => say("net.found", status.blocks_mined),
        Kind::Pool => say("net.members", line.workers.len() as u64),
        Kind::Full => say("net.holds_all", status.height),
        Kind::Pruned if expert => say("net.holds_recent", line.oldest_held),
        Kind::Pruned => say("net.holds_recent", status.height.saturating_sub(line.oldest_held) + 1),
        Kind::Light => say("net.holds_summary", status.headers.unwrap_or(status.height)),
        Kind::Receiver => say("net.received", line.incoming.len() as u64),
        Kind::Spammer => say("net.spam_sent", line.spam_sent),
        Kind::Sender | Kind::Exploiter => match member.waits {
            Some(key) => text(voice, key).to_string(),
            None if member.kind == Kind::Exploiter => say("net.refused_count", member.tricks.iter().filter(|row| row.refused).count() as u64),
            None => String::new(),
        },
    };
    let colour = if member.kind == Kind::Spammer && line.waiting_bytes > line.max_block_bytes { t.warn } else { t.muted };
    (shown, colour)
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
                let paid = paid_by_pool(state, member.id);
                facts.push(with("net.fact_in_pool", &[&name(voice, pool), &coins(paid)]));
            }
            None => facts.push(with("net.fact_miner", &[&status.blocks_mined.to_string(), &coins(status.balance)])),
        },
        Kind::Attacker => {
            let total: u64 = state.members.iter().filter_map(|other| other.status.as_ref()).map(|other| other.hashrate).sum();
            let share = (status.hashrate * 100).checked_div(total).unwrap_or(0);
            facts.push(with("net.fact_attacker", &[&share.to_string(), &coins(status.balance)]));
        }
        Kind::Pool => {
            facts.push(with("net.fact_pool", &[&line.workers.len().to_string(), &line.pool_found.to_string()]));
            for worker in line.workers.iter().take(2) {
                let who = state
                    .members
                    .iter()
                    .find(|other| other.address() == Some(worker.address.as_str()))
                    .map_or_else(|| worker.address.chars().take(10).collect(), |other| name(voice, other));
                facts.push(with("net.fact_worker", &[&who, &worker.accepted.to_string(), &coins(worker.paid)]));
            }
        }
        Kind::Full => facts.push(with("net.fact_full", &[&status.height.to_string()])),
        Kind::Pruned => facts.push(with("net.fact_pruned", &[&line.oldest_held.to_string()])),
        Kind::Light => facts.push(with("net.fact_light", &[&status.headers.unwrap_or(status.height).to_string()])),
        Kind::Sender => {
            let sent = state.events.iter().filter(|event| matches!(event.what, What::Paid { from, .. } if from == member.id)).count();
            facts.push(with("net.fact_sender", &[&sent.to_string(), &coins(status.balance)]));
        }
        Kind::Receiver => {
            let settled = state.events.iter().filter(|event| matches!(event.what, What::Settled { at, .. } if at == member.id)).count();
            let undone = line.incoming.iter().filter(|payment| payment.gone).count();
            facts.push(with("net.fact_receiver", &[&line.incoming.len().to_string(), &settled.to_string(), &undone.to_string()]));
        }
        Kind::Spammer => facts.push(with("net.fact_spammer", &[&line.spam_sent.to_string(), &(line.waiting_bytes / 1024).to_string()])),
        Kind::Exploiter => {
            for trick in crate::local::lab_net::Trick::ALL {
                let rows: Vec<_> = member.tricks.iter().filter(|row| row.trick == trick).collect();
                if rows.is_empty() {
                    continue;
                }
                let refused = rows.iter().filter(|row| row.refused).count();
                facts.push(with("net.fact_trick", &[text(voice, trick.key()), &rows.len().to_string(), &refused.to_string()]));
            }
        }
    }
    if let Some(key) = member.waits {
        facts.insert(0, text(voice, key).to_string());
    }
    facts.truncate(3);
    facts
}

fn paid_by_pool(state: &NetState, miner: u32) -> u64 {
    state.events.iter().filter_map(|event| match event.what {
        What::Payout { to, amount, .. } if to == miner => Some(amount),
        _ => None,
    }).sum()
}
