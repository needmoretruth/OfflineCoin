//! The newest moments of the network under the map, as sentences: who found a block, who paid
//! whom, which payment settled or was undone, whose blocks were replaced, and which trick which
//! nodes refused and by what rule. The map keeps only a short mark on the token.
//!
//! Blocks and payments come often, so only the newest of them is a line. When several nodes do the
//! same thing together (refuse one trick, drop the same blocks) that is one line naming them, and a
//! warning stays a few seconds however much happens after it, so a person can read it.
use super::lab_tokens::name;
use crate::i18n::{fill, text, Lang, Voice};
use crate::local::lab_net::{Kind, Member, NetState, What};
use crate::settings::Mode;
use crate::ui::tokens::{self, Tokens};
use egui::{RichText, Ui};

/// Sentences shown at once.
const SHOWN: usize = 4;
/// Seconds a warning stays among the lines however much happens after it.
const HOLD: f64 = 10.0;
/// Moments this close in order are one, when several nodes do the same thing.
const TOGETHER: u64 = 64;
/// Names said one by one; more become a count.
const NAMED: usize = 2;

/// What the lines remember between frames: when each recent warning first showed.
#[derive(Default)]
pub struct Said {
    seen: u64,
    warned: Vec<(u64, f64)>,
}

/// Moments told as one line: the first of them, newest first, and who else did the same.
struct Line<'a> {
    newest: u64,
    what: &'a What,
    who: Vec<u32>,
    amount: u64,
}

fn coins(voice: Voice, units: u64) -> String {
    super::lab_tokens::coins(voice, units)
}

/// A name as the one who did it: Korean adds 이 or 가 by how the name ends.
fn subject(voice: Voice, name: &str) -> String {
    if voice.lang != Lang::Ko {
        return name.to_string();
    }
    let ends_closed = match name.chars().last() {
        // Read as a number: 이, 사, 오 and 구 end open.
        Some(digit @ '0'..='9') => !matches!(digit, '2' | '4' | '5' | '9'),
        Some(c @ '가'..='힣') => (c as u32 - '가' as u32) % 28 != 0,
        _ => true,
    };
    format!("{name}{}", if ends_closed { "이" } else { "가" })
}

/// English trick lines start a sentence of their own; after a name they go on in lower case.
fn going_on(voice: Voice, words: &str) -> String {
    if voice.lang == Lang::Ko {
        return words.to_string();
    }
    let mut chars = words.chars();
    chars.next().map_or_else(String::new, |first| first.to_lowercase().chain(chars).collect())
}

fn routine(what: &What) -> bool {
    matches!(what, What::Block { .. } | What::Paid { .. } | What::Payout { .. })
}

/// Whether a moment is told at all. Every node counts the coins it was sent, its own block rewards
/// and pool payouts too, but the payment that settles or is undone is the one a receiver was sent:
/// the rest would bury it.
pub(super) fn told(state: &NetState, what: &What) -> bool {
    match what {
        What::Settled { at, .. } | What::Undone { at, .. } => state.members.iter().any(|member| member.id == *at && member.kind == Kind::Receiver),
        What::Shares { .. } => false,
        _ => true,
    }
}

fn warning(what: &What) -> bool {
    matches!(what, What::Undone { .. } | What::Replaced { .. })
}

/// Whether two moments are told as one, and who the later one adds.
fn joins(line: &What, other: &What) -> Option<u32> {
    match (line, other) {
        (What::Replaced { blocks, .. }, What::Replaced { at, blocks: same }) if blocks == same => Some(*at),
        (What::Refused { from, trick, .. }, What::Refused { from: same_from, by, trick: same_trick, .. }) => {
            (from == same_from && trick == same_trick).then_some(*by)
        }
        (What::Settled { at, .. }, What::Settled { at: same, .. }) | (What::Undone { at, .. }, What::Undone { at: same, .. }) => {
            (at == same).then_some(*at)
        }
        _ => None,
    }
}

fn amount(what: &What) -> u64 {
    match what {
        What::Settled { amount, .. } | What::Undone { amount, .. } => *amount,
        _ => 0,
    }
}

fn doer(what: &What) -> Option<u32> {
    match what {
        What::Replaced { at, .. } => Some(*at),
        What::Refused { by, .. } => Some(*by),
        _ => None,
    }
}

/// The moments as lines, newest first.
fn lines(state: &NetState) -> Vec<Line<'_>> {
    let mut lines: Vec<Line> = Vec::new();
    let mut told_routine = false;
    for event in state.events.iter().rev() {
        let what = &event.what;
        if !told(state, what) {
            continue;
        }
        if routine(what) {
            if !told_routine && !matches!(what, What::Block { finder: None, .. } | What::Paid { to: None, .. }) {
                told_routine = true;
                lines.push(Line { newest: event.seq, what, who: Vec::new(), amount: 0 });
            }
            continue;
        }
        let near = lines.iter_mut().filter(|line| line.newest - event.seq <= TOGETHER);
        let mut joined = false;
        for line in near {
            if let Some(who) = joins(line.what, what) {
                if !line.who.contains(&who) {
                    line.who.insert(0, who);
                }
                line.amount += amount(what);
                joined = true;
                break;
            }
        }
        if !joined {
            lines.push(Line { newest: event.seq, what, who: doer(what).into_iter().collect(), amount: amount(what) });
        }
    }
    lines
}

/// Names as one subject: two said, more counted.
fn names(voice: Voice, state: &NetState, who: &[u32]) -> Option<String> {
    let named: Vec<String> = who.iter().filter_map(|id| state.members.iter().find(|member: &&Member| member.id == *id)).map(|member| name(voice, member)).collect();
    let first = named.first()?;
    let said = match named.len() {
        count if count > NAMED => fill(text(voice, "net.say_more"), &[first, &(count - 1).to_string()]),
        _ => named.join(", "),
    };
    Some(subject(voice, &said))
}

/// The sentence for one line, and whether it is a warning.
fn sentence(voice: Voice, state: &NetState, line: &Line) -> Option<(String, bool)> {
    let named = |id: u32| state.members.iter().find(|member: &&Member| member.id == id).map(|member| name(voice, member));
    let said = match line.what {
        What::Block { finder: Some(finder), height, .. } => fill(text(voice, "net.say_block"), &[&subject(voice, &named(*finder)?), &height.to_string()]),
        What::Paid { from, to: Some(to), amount } => fill(text(voice, "net.say_paid"), &[&subject(voice, &named(*from)?), &named(*to)?, &coins(voice, *amount)]),
        What::Payout { from, to, amount } => fill(text(voice, "net.say_payout"), &[&subject(voice, &named(*from)?), &named(*to)?, &coins(voice, *amount)]),
        What::Settled { at, .. } => fill(text(voice, "net.say_settled"), &[&named(*at)?, &coins(voice, line.amount)]),
        What::Undone { at, .. } => fill(text(voice, "net.say_undone"), &[&named(*at)?, &coins(voice, line.amount)]),
        What::Replaced { blocks, .. } => fill(text(voice, "net.say_replaced"), &[&names(voice, state, &line.who)?, &blocks.to_string()]),
        What::Refused { from, trick, rule, .. } => {
            let mut why = text(voice, trick.rule_key()).to_string();
            if voice.mode == Mode::Expert && !rule.is_empty() {
                why = format!("{why} ({rule})");
            }
            let did = going_on(voice, text(voice, trick.key()));
            fill(text(voice, "net.say_refused"), &[&subject(voice, &named(*from)?), &did, &names(voice, state, &line.who)?, &why])
        }
        _ => return None,
    };
    Some((said, warning(line.what)))
}

/// The newest few sentences, oldest first, the newest brightest, a recent warning kept.
pub fn draw(ui: &mut Ui, t: &Tokens, voice: Voice, said: &mut Said, state: &NetState) {
    let now = ui.input(|input| input.time);
    let newest = state.events.last().map_or(0, |event| event.seq);
    if newest < said.seen {
        *said = Said::default();
    }
    for event in state.events.iter().filter(|event| event.seq > said.seen && warning(&event.what) && told(state, &event.what)) {
        said.warned.push((event.seq, now));
    }
    said.seen = said.seen.max(newest);
    said.warned.retain(|(_, when)| now - when < HOLD);
    let all = lines(state);
    let held = |line: &Line| said.warned.iter().any(|(seq, _)| *seq == line.newest);
    let mut chosen: Vec<&Line> = all.iter().filter(|line| held(line)).take(SHOWN).collect();
    for line in all.iter().filter(|line| !held(line)) {
        if chosen.len() >= SHOWN {
            break;
        }
        chosen.push(line);
    }
    chosen.sort_by_key(|line| line.newest);
    let shown: Vec<(String, bool)> = chosen.into_iter().filter_map(|line| sentence(voice, state, line)).collect();
    let top = ui.cursor().top();
    let last = shown.len().saturating_sub(1);
    for (index, (line, warn)) in shown.into_iter().enumerate() {
        let colour = match (warn, index == last) {
            (true, _) => t.warn,
            (false, true) => t.text,
            (false, false) => t.muted,
        };
        ui.label(RichText::new(line).color(colour).size(tokens::META));
    }
    // The same room whether few or many moments came, so the page does not jump.
    let used = ui.cursor().top() - top;
    let room = SHOWN as f32 * (tokens::META + 6.0);
    if used < room {
        ui.add_space(room - used);
    }
}
