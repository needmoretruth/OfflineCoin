//! One plain sentence for each step of a Lab run.
use super::lab_tokens::number;
use crate::i18n::{fill, text, Voice};
use crate::local::lab::{LabKind, LabState, Phase};
use olc_lab::Step;

fn percent(part: u64, other: u64) -> (String, String) {
    let total = part + other;
    if total == 0 {
        return ("0".into(), "0".into());
    }
    let first = (part * 100 + total / 2) / total;
    (first.to_string(), (100 - first).to_string())
}

pub fn step_words(voice: Voice, kind: LabKind, step: &Step) -> String {
    let say = |key: &str| text(voice, key).to_string();
    let with = |key: &str, values: &[&str]| fill(text(voice, key), values);
    match (kind, step) {
        (LabKind::Attack, Step::Power { attacker, victim }) => {
            let (a, v) = percent(*attacker, *victim);
            with("lab2.step_power", &[&a, &v])
        }
        (LabKind::Split, Step::Power { attacker, victim }) => {
            let (a, v) = percent(*attacker, *victim);
            with("lab2.step_split_power", &[&a, &v])
        }
        (LabKind::Attack, Step::LinedUp { .. }) => say("lab2.step_lined"),
        (LabKind::Split, Step::LinedUp { .. }) => say("lab2.step_split_lined"),
        (_, Step::Paid { .. }) => say("lab2.step_paid"),
        (_, Step::Dark) => say("lab2.step_dark"),
        (_, Step::Confirmed { depth, of }) => with("lab2.step_confirmed", &[&number(*depth), &number(*of)]),
        (_, Step::SpentAgain { .. }) => say("lab2.step_spent"),
        (_, Step::Heavier) => say("lab2.step_heavier"),
        (_, Step::GaveUp { behind }) => with("lab2.step_gave_up", &[&number(*behind)]),
        (_, Step::Revealed) => say("lab2.step_revealed"),
        (_, Step::Replaced { blocks, hidden_won: true, .. }) => with("lab2.step_replaced_won", &[&number(*blocks)]),
        (_, Step::Replaced { blocks, hidden_won: false, .. }) => with("lab2.step_replaced_lost", &[&number(*blocks)]),
        (_, Step::Refused) => say("lab2.step_refused"),
        (_, Step::Gone) => say("lab2.step_gone"),
        (_, Step::Stays) => say("lab2.step_stays"),
        (_, Step::Cut { .. }) => say("lab2.step_cut"),
        (_, Step::Rejoined) => say("lab2.step_rejoined"),
        (_, Step::Settled { left_won: true, blocks, .. }) => with("lab2.step_settled_left", &[&number(*blocks)]),
        (_, Step::Settled { left_won: false, blocks, .. }) => with("lab2.step_settled_right", &[&number(*blocks)]),
    }
}

/// Why a run stopped, in the person's words. The experiments say it in English for the console;
/// an expert sees that line too.
fn why(voice: Voice, error: &str) -> String {
    let lower = error.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|word| lower.contains(word));
    let key = if has(&["stopped by hand"]) {
        "net.why_stopped"
    } else if has(&["out of time"]) {
        "net.why_time"
    } else if has(&["not_enough", "holds only", "did not pay"]) {
        "net.why_coins"
    } else if has(&["did not reach"]) {
        "net.why_reach"
    } else if has(&["did not agree", "changed branch"]) {
        "net.why_agree"
    } else if has(&["needs a miner", "miners are needed", "boards are needed"]) {
        "net.why_miners"
    } else {
        "net.why_other"
    };
    match voice.mode {
        crate::settings::Mode::Expert if !error.is_empty() => format!("{} ({error})", text(voice, key)),
        _ => text(voice, key).to_string(),
    }
}

/// The one sentence that says where a run stands now.
pub fn now_words(voice: Voice, state: &LabState) -> String {
    let kind = state.kind.unwrap_or(LabKind::Attack);
    match state.phase {
        Phase::Idle | Phase::Starting => text(voice, "lab2.starting").to_string(),
        Phase::Warming => text(voice, "lab2.warming").to_string(),
        Phase::Failed => fill(text(voice, "lab2.failed"), &[&why(voice, state.error.as_deref().unwrap_or(""))]),
        Phase::Running | Phase::Done => match state.frames.last() {
            Some(frame) => step_words(voice, kind, &frame.step),
            None => text(voice, "lab2.starting").to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Lang;
    use crate::settings::Mode;

    #[test]
    fn every_step_reads_in_both_languages_and_modes() {
        let steps = [
            Step::Power { attacker: 18_000, victim: 6_000 },
            Step::LinedUp { height: 3 },
            Step::Paid { payment: "ab".into() },
            Step::Dark,
            Step::Confirmed { depth: 1, of: 2 },
            Step::SpentAgain { spend: "cd".into() },
            Step::Heavier,
            Step::GaveUp { behind: 6 },
            Step::Revealed,
            Step::Replaced { height: 5, blocks: 3, hidden_won: true },
            Step::Replaced { height: 5, blocks: 3, hidden_won: false },
            Step::Refused,
            Step::Gone,
            Step::Stays,
            Step::Cut { height: 4 },
            Step::Rejoined,
            Step::Settled { left_won: true, height: 9, blocks: 2 },
            Step::Settled { left_won: false, height: 9, blocks: 2 },
        ];
        for lang in [Lang::En, Lang::Ko] {
            for mode in [Mode::Basic, Mode::Expert] {
                for kind in [LabKind::Attack, LabKind::Split] {
                    for step in &steps {
                        let words = step_words(Voice { lang, mode }, kind, step);
                        assert!(!words.contains('?') && !words.contains("{}"), "{words}");
                    }
                }
            }
        }
        let basic = Voice { lang: Lang::En, mode: Mode::Basic };
        assert_eq!(step_words(basic, LabKind::Attack, &steps[0]), "The attacker mines with 75% of the power and the victim with 25%.");
    }
}
