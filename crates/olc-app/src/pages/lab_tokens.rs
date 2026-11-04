//! What each participant is called and the few facts it says: one short line under its token,
//! and at most three lines in its panel. Basic mode counts in plain words; expert mode says
//! heights, hash rates and the mempool.
use crate::i18n::{fill, text, Voice};
use crate::local::lab_net::{Kind, Member, NetState, Trick};
use crate::settings::Mode;
use crate::ui::format;
use crate::ui::tokens::Tokens;
use egui::Color32;

/// A count, a height or a speed as the app writes every number, its thousands marked.
pub fn number(value: impl TryInto<u128>) -> String {
    format::number(value.try_into().unwrap_or_default())
}

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

/// Its kind and its number among its kind: "Miner 2", or "Bad pool 1" once a pool turned bad.
pub fn name(voice: Voice, member: &Member) -> String {
    let kind = if member.kind == Kind::Pool && member.bad { "net.kind_bad_pool" } else { kind_key(member.kind) };
    format!("{} {}", text(voice, kind), member.number)
}

/// The share of all hashing an attacker really has now, in percent, as the nodes measure it: a
/// bad miner's own, a bad pool's miners'. `None` while nobody hashes.
pub fn power(member: &Member, state: &NetState) -> Option<u64> {
    let rate = |other: &Member| other.status.as_ref().map_or(0, |status| status.hashrate);
    let total: u64 = state.members.iter().filter(|other| other.kind.mines()).map(rate).sum();
    let own: u64 = match member.kind {
        Kind::Pool => state.members.iter().filter(|other| other.pool == Some(member.id)).map(rate).sum(),
        _ => rate(member),
    };
    (own * 100).checked_div(total)
}

/// The rule an attacker's share answers to, plainly: over half replaces blocks, under half
/// rarely catches up. A pool has no speed of its own: its miners are its power.
fn rule(voice: Voice, share: u64, pool: bool) -> String {
    let under = if pool { "net.rule_under_pool" } else { "net.rule_under" };
    text(voice, if share > 50 { "net.rule_over" } else { under }).to_string()
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
pub fn status_line(t: &Tokens, voice: Voice, member: &Member, state: &NetState) -> (String, Color32) {
    if member.ended.is_some() {
        return (text(voice, "net.ended").to_string(), t.bad);
    }
    if member.paused {
        let key = if member.kind.mines() || member.kind.speed().is_some() { "net.paused" } else { "net.cut_off" };
        return (text(voice, key).to_string(), t.muted);
    }
    let Some(status) = &member.status else {
        return (text(voice, "net.wait_start").to_string(), t.muted);
    };
    let attacker = member.kind == Kind::Attacker || (member.kind == Kind::Pool && member.bad);
    if attacker {
        let share = number(power(member, state).unwrap_or(0));
        let key = if member.busy { "net.attacking" } else { "net.power" };
        return (fill(text(voice, key), &[&share]), t.bad);
    }
    let line = lab(member);
    let expert = voice.mode == Mode::Expert;
    let say = |key: &str, value: u64| fill(text(voice, key), &[&number(value)]);
    let shown = match member.kind {
        // In a pool the pool finds the blocks: a member counts the shares it handed in.
        Kind::Miner if member.pool.is_some() => say("net.shares", line.accepted),
        Kind::Miner | Kind::Attacker if expert => format!("{} · {}", format::rate(voice, status.hashrate), say("net.found", status.blocks_mined)),
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
        let counted = fill(text(voice, "net.fact_trick"), &[&number(rows.len()), &number(refused)]);
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
            None => facts.push(with("net.fact_miner", &[&number(status.blocks_mined), &coins(voice, status.balance)])),
        },
        Kind::Attacker => {
            let share = power(member, state).unwrap_or(0);
            facts.push(with("net.fact_attacker", &[&number(share), &coins(voice, status.balance)]));
            facts.push(rule(voice, share, false));
        }
        Kind::Pool if member.bad => {
            let share = power(member, state).unwrap_or(0);
            let crew = state.members.iter().filter(|other| other.pool == Some(member.id)).count();
            facts.push(with("net.fact_bad_pool", &[&number(crew), &number(share)]));
            facts.push(rule(voice, share, true));
            facts.push(with("net.fact_held", &[&coins(voice, status.balance)]));
        }
        Kind::Pool => {
            facts.push(with("net.fact_pool", &[&number(line.workers.len()), &number(line.pool_found)]));
            for worker in line.workers.iter().take(2) {
                let who = state
                    .members
                    .iter()
                    .find(|other| other.address() == Some(worker.address.as_str()))
                    .map_or_else(|| worker.address.chars().take(10).collect(), |other| name(voice, other));
                facts.push(with("net.fact_worker", &[&who, &number(worker.accepted), &coins(voice, worker.paid)]));
            }
        }
        Kind::Full => facts.push(with("net.fact_full", &[&number(status.height)])),
        Kind::Pruned => facts.push(with("net.fact_pruned", &[&number(line.oldest_held)])),
        Kind::Light => facts.push(with("net.fact_light", &[&number(status.headers.unwrap_or(status.height))])),
        Kind::Sender => facts.push(with("net.fact_sender", &[&number(member.received.sent), &coins(voice, status.balance)])),
        Kind::Receiver => {
            let got = member.received;
            facts.push(with("net.fact_receiver", &[&number(got.payments), &number(got.settled), &number(got.undone)]));
        }
        Kind::Spammer => facts.push(with("net.fact_spammer", &[&number(line.spam_sent), &number(line.waiting_bytes.div_ceil(1024))])),
        Kind::Exploiter => {
            let rows = &member.tricks;
            if let Some(newest) = rows.last() {
                let tried = Trick::ALL.iter().filter(|trick| rows.iter().any(|row| row.trick == **trick)).count();
                let nodes: std::collections::BTreeSet<u32> = rows.iter().map(|row| row.by).collect();
                let refused = rows.iter().filter(|row| row.refused).count();
                facts.push(with("net.fact_tricks", &[&number(tried), &number(nodes.len()), &number(refused)]));
                facts.push(text(voice, newest.trick.key()).to_string());
                let through = rows.iter().filter(|row| !row.refused && row.failed.is_none()).count();
                let failed = rows.iter().filter(|row| row.failed.is_some()).count();
                facts.push(match (through, failed) {
                    (1.., _) => with("net.trick_let_through", &[&number(through)]),
                    (0, 1..) => with("net.trick_failed", &[&number(failed)]),
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


#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Lang;
    use crate::local::lab_net::Received;
    use crate::local::ChildStatus;

    fn member(kind: Kind, status: &ChildStatus) -> Member {
        Member {
            id: 1,
            kind,
            number: 1,
            speed: 40_000,
            limit: 12_500,
            paused: false,
            pool: None,
            links: Vec::new(),
            status: Some(status.clone()),
            pid: None,
            ended: None,
            waits: None,
            tricks: Vec::new(),
            busy: false,
            bad: false,
            raced: false,
            received: Received { payments: 2_048, ..Received::default() },
        }
    }

    /// Speeds, heights, block and payment counts read as every other page writes numbers.
    #[test]
    fn every_lab_number_has_its_thousands_marked() {
        let status = ChildStatus { height: 12_345, blocks_mined: 1_234, hashrate: 12_500, ..ChildStatus::default() };
        let state = NetState::default();
        let t = crate::ui::tokens::DARK;
        for lang in [Lang::En, Lang::Ko] {
            for mode in [Mode::Basic, Mode::Expert] {
                let voice = Voice { lang, mode };
                let said = |kind: Kind| {
                    let member = member(kind, &status);
                    format!("{} | {}", status_line(&t, voice, &member, &state).0, facts(voice, &member, &state).join(" "))
                };
                for (kind, figure) in [(Kind::Full, "12,345"), (Kind::Miner, "1,234"), (Kind::Receiver, "2,048")] {
                    let words = said(kind);
                    assert!(words.contains(figure), "{lang:?} {mode:?} {kind:?}: {words}");
                }
                if mode == Mode::Expert {
                    let words = said(Kind::Miner);
                    assert!(words.contains("12,500"), "{lang:?} expert miner: {words}");
                }
            }
        }
    }
}
