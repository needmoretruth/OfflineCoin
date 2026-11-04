//! What each participant is called and the few facts it says: one short line under its token,
//! and at most three lines in its panel. Basic mode counts in plain words; expert mode says
//! heights, hash rates and the mempool.
use crate::i18n::{text, Voice};
use super::lab_words::fill;
use crate::local::lab_net::{Kind, Member, NetState, Trick, TrickRow};
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

/// How many tries could not be made and on whom; an expert also reads the newest cause.
fn failed_on<'a>(voice: Voice, rows: impl Iterator<Item = &'a TrickRow>, state: &NetState) -> String {
    let failed: Vec<_> = rows.filter(|row| row.failed.is_some()).collect();
    let on: std::collections::BTreeSet<u32> = failed.iter().map(|row| row.by).collect();
    let names: Vec<String> = on.iter().filter_map(|id| state.members.iter().find(|member| member.id == *id)).map(|member| name(voice, member)).collect();
    let said = fill(text(voice, "net.trick_failed"), &[&number(failed.len()), &names.join(", ")]);
    match failed.last().and_then(|row| row.failed.as_deref()) {
        Some(cause) if voice.mode == Mode::Expert => format!("{said} ({cause})"),
        _ => said,
    }
}

/// What a wait is called under a token, where a cell's width holds a few words; the panel says
/// the whole sentence.
fn wait_word(key: &'static str) -> &'static str {
    match key {
        "net.wait_coins" => "net.tok_wait_coins",
        "net.wait_miner" => "net.tok_wait_miner",
        "net.wait_receiver" => "net.tok_wait_receiver",
        "net.wait_target" => "net.tok_wait_target",
        "net.wait_run" => "net.tok_wait_run",
        other => other,
    }
}

/// The line under a token, and its colour. An expert miner's has two rows: its speed above the
/// blocks it found.
pub fn status_line(t: &Tokens, voice: Voice, member: &Member, state: &NetState) -> (String, Color32) {
    if member.ended.is_some() {
        return (text(voice, "net.tok_ended").to_string(), t.bad);
    }
    if member.paused {
        let key = if member.kind.mines() || member.kind.speed().is_some() { "net.paused" } else { "net.cut_off" };
        return (text(voice, key).to_string(), t.muted);
    }
    let Some(status) = &member.status else {
        return (text(voice, "net.wait_start").to_string(), t.muted);
    };
    let line = lab(member);
    // A miner that started on a tip over an hour old hears its neighbours for a few seconds
    // before it mines alone; a miner in a pool works for the pool all along.
    if member.kind.mines() && member.pool.is_none() && line.looking {
        return (text(voice, "net.tok_looking").to_string(), t.muted);
    }
    let attacker = member.kind == Kind::Attacker || (member.kind == Kind::Pool && member.bad);
    if attacker {
        let share = number(power(member, state).unwrap_or(0));
        let key = if member.busy { "net.attacking" } else { "net.power" };
        return (fill(text(voice, key), &[&share]), t.bad);
    }
    let expert = voice.mode == Mode::Expert;
    let say = |key: &str, value: u64| fill(text(voice, key), &[&number(value)]);
    let shown = match member.kind {
        // In a pool the pool finds the blocks: a member counts the shares it handed in, and an
        // expert reads the speed it hashes at above them, as for a miner alone.
        Kind::Miner if member.pool.is_some() && expert => format!("{}\n{}", format::rate(voice, status.hashrate), say("net.shares", line.accepted)),
        Kind::Miner if member.pool.is_some() => say("net.shares", line.accepted),
        Kind::Miner | Kind::Attacker if expert => format!("{}\n{}", format::rate(voice, status.hashrate), say("net.found", status.blocks_mined)),
        Kind::Miner | Kind::Attacker => say("net.found", status.blocks_mined),
        Kind::Pool => say("net.members", line.workers.len() as u64),
        Kind::Full => say("net.holds_all", status.height),
        Kind::Pruned if expert => say("net.holds_recent", line.oldest_held),
        Kind::Pruned => say("net.holds_recent", status.height.saturating_sub(line.oldest_held) + 1),
        Kind::Light => say("net.holds_summary", status.headers.unwrap_or(status.height)),
        Kind::Receiver => say("net.received", member.received.payments),
        Kind::Spammer => say("net.spam_sent", line.spam_sent),
        Kind::Sender | Kind::Exploiter => match member.waits {
            Some(key) => text(voice, wait_word(key)).to_string(),
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
        let mut said = format!("{counted} {}", text(voice, trick.rule_key()));
        if rows.iter().any(|row| row.failed.is_some()) {
            said = format!("{said} {}", failed_on(voice, rows.iter().copied(), state));
        }
        lines.push((text(voice, trick.key()).to_string(), said));
    }
    if !lines.is_empty() && state.members.iter().any(|other| other.kind == Kind::Light) {
        lines.push((String::new(), text(voice, "net.trick_skip_light").to_string()));
    }
    lines
}

/// What it holds, as Home says it: the coins it can spend, and a reward still too young to spend
/// as coming soon, so a block just found never reads as nothing held.
fn holding(voice: Voice, balance: u64, maturing: u64) -> String {
    let holds = (balance > 0 || maturing == 0).then(|| fill(text(voice, "net.holds"), &[&coins(voice, balance)]));
    let soon = (maturing > 0).then(|| fill(text(voice, "net.soon"), &[&coins(voice, maturing)]));
    holds.into_iter().chain(soon).collect::<Vec<_>>().join(" ")
}

/// At most three lines that say what it holds and does now.
pub fn facts(voice: Voice, member: &Member, state: &NetState) -> Vec<String> {
    let Some(status) = &member.status else {
        return vec![text(voice, "net.wait_start").to_string()];
    };
    let line = lab(member);
    let with = |key: &str, values: &[&str]| fill(text(voice, key), values);
    let holds = holding(voice, status.balance, line.maturing);
    let mut facts = Vec::new();
    match member.kind {
        Kind::Miner => match member.pool.and_then(|pool| state.members.iter().find(|other| other.id == pool)) {
            Some(pool) => {
                let paid = member.received.from_pool;
                facts.push(with("net.fact_in_pool", &[&name(voice, pool), &coins(voice, paid)]));
                // Shares of a job a new block ended are work lost to timing: an expert reads how many.
                if voice.mode == Mode::Expert {
                    facts.push(with("pro.stale_shares", &[&number(line.stale), &number(line.accepted + line.stale)]));
                }
            }
            None => facts.push(format!("{} {holds}", with("net.fact_miner", &[&number(status.blocks_mined)]))),
        },
        Kind::Attacker => {
            let share = power(member, state).unwrap_or(0);
            facts.push(format!("{} {holds}", with("net.fact_attacker", &[&number(share)])));
            facts.push(rule(voice, share, false));
        }
        Kind::Pool if member.bad => {
            let share = power(member, state).unwrap_or(0);
            let crew = state.members.iter().filter(|other| other.pool == Some(member.id)).count();
            facts.push(with("net.fact_bad_pool", &[&number(crew), &number(share)]));
            facts.push(rule(voice, share, true));
            facts.push(format!("{} {holds}", with("net.fact_held", &[])));
        }
        Kind::Pool => {
            facts.push(with("net.fact_pool", &[&number(line.workers.len()), &number(line.pool_found)]));
            for worker in line.workers.iter().take(2) {
                let who = state
                    .members
                    .iter()
                    .find(|other| other.address() == Some(worker.address.as_str()))
                    .map_or_else(|| worker.address.chars().take(10).collect(), |other| name(voice, other));
                let paid = coins(voice, worker.paid);
                facts.push(match voice.mode {
                    Mode::Expert => with("pro.worker_stale", &[&who, &number(worker.accepted), &number(worker.stale), &paid]),
                    Mode::Basic => with("net.fact_worker", &[&who, &number(worker.accepted), &paid]),
                });
            }
        }
        Kind::Full => facts.push(with("net.fact_full", &[&number(status.height)])),
        Kind::Pruned => facts.push(with("net.fact_pruned", &[&number(line.oldest_held)])),
        Kind::Light => facts.push(with("net.fact_light", &[&number(status.headers.unwrap_or(status.height))])),
        Kind::Sender => facts.push(format!("{} {holds}", with("net.fact_sender", &[&number(member.received.sent)]))),
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
                    (0, 1..) => failed_on(voice, rows.iter(), state),
                    _ => text(voice, newest.trick.rule_key()).to_string(),
                });
            }
        }
    }
    if let Some(key) = member.waits {
        facts.insert(0, text(voice, key).to_string());
    }
    // What the token says in a few words, the panel says whole.
    if member.kind.mines() && member.pool.is_none() && line.looking {
        facts.insert(0, text(voice, "net.fact_looking").to_string());
    }
    if member.ended.is_some() {
        facts.insert(0, text(voice, "net.ended").to_string());
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

    /// One of anything reads as one: "Refused 1 time", not "Refused 1 times".
    #[test]
    fn a_count_of_one_reads_in_the_singular() {
        let status = ChildStatus::default();
        let state = NetState::default();
        let t = crate::ui::tokens::DARK;
        let row = |refused: bool| TrickRow {
            trick: Trick::ALL[0],
            by: 2,
            refused,
            expected: refused,
            rule: String::new(),
            failed: None,
        };
        for mode in [Mode::Basic, Mode::Expert] {
            let voice = Voice { lang: Lang::En, mode };
            let said = |rows: Vec<TrickRow>| {
                let exploiter = Member { tricks: rows, ..member(Kind::Exploiter, &status) };
                format!("{} | {}", status_line(&t, voice, &exploiter, &state).0, facts(voice, &exploiter, &state).join(" "))
            };
            let one = said(vec![row(true)]);
            for many in ["1 times", "1 tricks", "1 others"] {
                assert!(!one.contains(many), "{mode:?}: {one}");
            }
            assert!(one.contains("1 time"), "{mode:?}: {one}");
            let two = said(vec![row(true), row(true)]);
            assert!(two.contains("2 times"), "{mode:?}: {two}");
        }
        assert_eq!(super::super::lab_words::fill("{} blocks, {} blocks", &["1", "21"]), "1 block, 21 blocks");
        assert_eq!(super::super::lab_words::fill("블록 {}개", &["1"]), "블록 1개");
    }

    /// A reward still too young to spend reads as Home says it, coming soon, never as nothing held,
    /// on every kind that says what it holds.
    #[test]
    fn a_reward_not_yet_spendable_reads_as_coming_not_as_nothing() {
        let soon = 5_000_000_000;
        let lab = crate::local::lab_side::LabLine { maturing: soon, ..Default::default() };
        let status = ChildStatus { blocks_mined: 1, lab: Some(lab), ..ChildStatus::default() };
        let state = NetState::default();
        let bad_pool = Member { bad: true, ..member(Kind::Pool, &status) };
        let holders = [member(Kind::Miner, &status), member(Kind::Attacker, &status), member(Kind::Sender, &status), bad_pool];
        for (lang, mode, word) in [
            (Lang::Ko, Mode::Basic, "곧 들어옵니다"),
            (Lang::En, Mode::Basic, "arrives soon"),
            (Lang::Ko, Mode::Expert, "미성숙"),
            (Lang::En, Mode::Expert, "immature"),
        ] {
            let voice = Voice { lang, mode };
            for holder in &holders {
                let said = facts(voice, holder, &state).join(" ");
                assert!(said.contains(word) && said.contains(&coins(voice, soon)), "{lang:?} {mode:?} {:?}: {said}", holder.kind);
                assert!(!said.contains(&format!(" {}", coins(voice, 0))), "{lang:?} {mode:?} {:?} holds nothing: {said}", holder.kind);
            }
        }
    }

    /// A miner that started on a tip over an hour old says so for the seconds it holds back.
    #[test]
    fn a_miner_on_an_old_tip_says_it_waits_for_its_neighbours() {
        let lab = crate::local::lab_side::LabLine { looking: true, ..Default::default() };
        let status = ChildStatus { lab: Some(lab), ..ChildStatus::default() };
        let state = NetState::default();
        let t = crate::ui::tokens::DARK;
        let voice = Voice { lang: Lang::Ko, mode: Mode::Basic };
        let alone = member(Kind::Miner, &status);
        assert_eq!(status_line(&t, voice, &alone, &state).0, "이웃 블록을 기다리는 중");
        let pooled = Member { pool: Some(9), ..member(Kind::Miner, &status) };
        assert_ne!(status_line(&t, voice, &pooled, &state).0, "이웃 블록을 기다리는 중");
    }

    /// In expert mode a miner in a pool shows the speed it hashes at, as a miner alone does, with
    /// the shares it handed in under it; a plain reader sees the shares.
    #[test]
    fn a_pool_member_shows_its_speed_and_its_shares_in_expert_mode() {
        let lab = crate::local::lab_side::LabLine { accepted: 1_234, ..Default::default() };
        let status = ChildStatus { hashrate: 6_573, lab: Some(lab), ..ChildStatus::default() };
        let state = NetState::default();
        let t = crate::ui::tokens::DARK;
        let pooled = Member { pool: Some(9), ..member(Kind::Miner, &status) };
        for lang in [Lang::En, Lang::Ko] {
            let line = |mode| status_line(&t, Voice { lang, mode }, &pooled, &state).0;
            let expert = line(Mode::Expert);
            let rows: Vec<&str> = expert.lines().collect();
            assert!(rows.len() == 2 && rows[0].contains("6,573") && rows[1].contains("1,234"), "{lang:?} expert pool member: {expert:?}");
            let basic = line(Mode::Basic);
            assert!(basic.contains("1,234") && !basic.contains("6,573"), "{lang:?} plain pool member: {basic:?}");
        }
    }

    /// Each row of the line under a token is at most as wide as its cell. A state it says alone, and a
    /// count as high as a Lab run reaches, stand whole there in both languages and both modes.
    #[test]
    fn every_token_line_stands_whole_under_its_token() {
        use crate::pages::lab_places::{LINE_SIZE, WORDS_WIDTH};
        let ctx = egui::Context::default();
        ctx.set_fonts(crate::ui::fonts::definitions());
        ctx.begin_pass(egui::RawInput::default());
        let font = egui::FontId::proportional(LINE_SIZE);
        let width = |words: &str| ctx.fonts_mut(|fonts| fonts.layout_no_wrap(words.to_string(), font.clone(), Color32::WHITE).size().x);
        let lab = crate::local::lab_side::LabLine { accepted: 1_234, spam_sent: 1_234, oldest_held: 1_234, ..Default::default() };
        let status = ChildStatus { height: 1_234, blocks_mined: 999, hashrate: 134_596, lab: Some(lab), ..ChildStatus::default() };
        let state = NetState::default();
        let t = crate::ui::tokens::DARK;
        let mut cut = Vec::new();
        for lang in [Lang::En, Lang::Ko] {
            for mode in [Mode::Basic, Mode::Expert] {
                let voice = Voice { lang, mode };
                let said = |member: &Member, state: &NetState| status_line(&t, voice, member, state).0;
                let mut lines: Vec<String> = Kind::ALL.iter().map(|kind| said(&member(*kind, &status), &state)).collect();
                lines.push(said(&Member { pool: Some(9), ..member(Kind::Miner, &status) }, &state));
                let racing = Member { busy: true, ..member(Kind::Attacker, &status) };
                lines.push(said(&racing, &NetState { members: vec![racing.clone()], ..NetState::default() }));
                let alone = ["net.tok_ended", "net.paused", "net.cut_off", "net.wait_start", "net.tok_looking"];
                let waits = ["net.wait_coins", "net.wait_receiver", "net.wait_target", "net.wait_miner", "net.wait_run"];
                lines.extend(alone.into_iter().chain(waits.into_iter().map(wait_word)).map(|key| text(voice, key).to_string()));
                for line in lines.iter().flat_map(|line| line.lines()) {
                    let wide = width(line);
                    if wide > WORDS_WIDTH {
                        cut.push(format!("{lang:?} {mode:?}: {line} ({wide:.0} of {WORDS_WIDTH:.0})"));
                    }
                }
            }
        }
        ctx.end_pass().textures_delta.clear();
        assert!(cut.is_empty(), "cut under a token:\n{}", cut.join("\n"));
    }
}
