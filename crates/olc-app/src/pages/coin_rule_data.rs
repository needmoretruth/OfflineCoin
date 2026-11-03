//! The blocks behind the difficulty picture: for each rule and each change of mining, when every
//! block came after the change and whether it was an easy block at the floor. Nine real nodes wrote
//! them, one a rule and change (`crates/olc-app/data/difficulty-drop.json`, made by the difficulty
//! measurement and described in `docs/technical.md`). A block before the change has a time below 0.
use serde_json::Value;
use std::sync::LazyLock;

const FILE: &str = include_str!("../../data/difficulty-drop.json");

/// Without the file's own answer, the pace is back where this many gaps in a row average within
/// half again of the target either way, with no easy block among them.
const PACE_GAPS: u8 = 5;
const PACE_SLACK: f64 = 1.5;

pub struct Block {
    pub secs: f64,
    /// Took the floor because it came late, as Bitcoin's test network allows.
    pub easy: bool,
}

pub struct Run {
    pub rule: String,
    pub case: String,
    pub target: f64,
    pub blocks: Vec<Block>,
    /// Seconds after the change when blocks came at their pace again, if they did while watched.
    pub back: Option<f64>,
    /// How long after the change the run was watched, when the file says.
    pub watched: Option<f64>,
    /// Whether the run let a late block come easy, when the file says.
    pub late_floor: Option<bool>,
}

impl Run {
    /// The first block after the change.
    pub fn first(&self) -> Option<f64> {
        self.blocks.iter().map(|block| block.secs).find(|secs| *secs >= 0.0)
    }

    /// Blocks after the change up to and with the one where the pace was back.
    pub fn blocks_to_back(&self) -> Option<usize> {
        let back = self.back?;
        Some(self.blocks.iter().filter(|block| (0.0..=back).contains(&block.secs)).count())
    }
}

static RUNS: LazyLock<Vec<Run>> = LazyLock::new(|| read(FILE));

/// The changes measured, in the file's order: a fall of mining first when there is one.
pub fn cases() -> Vec<&'static str> {
    let mut cases: Vec<&'static str> = Vec::new();
    for run in RUNS.iter() {
        if !cases.contains(&run.case.as_str()) {
            cases.push(run.case.as_str());
        }
    }
    cases
}

pub fn run(rule: &str, case: &str) -> Option<&'static Run> {
    RUNS.iter().find(|run| run.rule == rule && run.case == case)
}

/// The earliest block of a change, 0 when none came before it.
pub fn earliest(case: &str) -> f64 {
    let blocks = RUNS.iter().filter(|run| run.case == case).flat_map(|run| &run.blocks);
    blocks.map(|block| block.secs).fold(0.0, f64::min)
}

/// How many times mining fell in a case, `drop20` twenty: the first block's wait then grows as
/// many times unless a late block may come easy. None for a rise.
pub fn drop_factor(case: &str) -> Option<f64> {
    case.strip_prefix("drop")?.parse::<u32>().ok().map(f64::from)
}

/// What a case's name says: `drop20` is mining falling to a twentieth, `rise50` growing fifty
/// times. The word and the number, or None when the name says neither.
pub fn case_words(case: &str) -> Option<(&'static str, &str)> {
    let (key, number) = if let Some(number) = case.strip_prefix("drop") {
        ("coin.case_drop", number)
    } else {
        ("coin.case_rise", case.strip_prefix("rise")?)
    };
    (!number.is_empty() && number.chars().all(|c| c.is_ascii_digit())).then_some((key, number))
}

fn read(text: &str) -> Vec<Run> {
    let Ok(file) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let runs = file.get("runs").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    runs.iter().filter_map(run_of).collect()
}

fn run_of(value: &Value) -> Option<Run> {
    let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_string);
    let target = value.get("target_secs").and_then(Value::as_f64).filter(|secs| *secs > 0.0)?;
    let mut blocks: Vec<Block> = value
        .get("blocks")?
        .as_array()?
        .iter()
        .filter_map(|block| {
            let secs = block.get("secs_after_change")?.as_f64()?;
            Some(Block { secs, easy: block.get("min_difficulty").and_then(Value::as_bool).unwrap_or(false) })
        })
        .collect();
    blocks.sort_by(|a, b| a.secs.total_cmp(&b.secs));
    // The file's null is its answer: not back while watched. Only a file without the key leaves
    // the pace to be read from the blocks.
    let back = match value.get("back_secs") {
        Some(back) => back.as_f64(),
        None => back_by_pace(&blocks, target),
    };
    let watched = value.get("watched_secs").and_then(Value::as_f64);
    let late_floor = value.get("allow_min_difficulty_blocks").and_then(Value::as_bool);
    Some(Run { rule: text("rule")?, case: text("case")?, target, blocks, back, watched, late_floor })
}

/// The first regular block from which [`PACE_GAPS`] gaps average within [`PACE_SLACK`] of the
/// target, with no easy block among them.
fn back_by_pace(blocks: &[Block], target: f64) -> Option<f64> {
    (0..blocks.len()).find_map(|start| {
        let window = blocks.get(start..=start + usize::from(PACE_GAPS))?;
        let (first, last) = (window.first()?, window.last()?);
        let mean = (last.secs - first.secs) / f64::from(PACE_GAPS);
        let steady = mean >= target / PACE_SLACK && mean <= target * PACE_SLACK;
        (steady && window.iter().all(|block| !block.easy)).then_some(first.secs)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::DIFFICULTY_RULES;

    #[test]
    fn every_rule_has_its_blocks_for_every_change_measured() {
        let cases = cases();
        assert!(!cases.is_empty(), "the file reads");
        for case in cases {
            assert!(case_words(case).is_some(), "{case} names a fall or a rise");
            for rule in DIFFICULTY_RULES {
                let run = run(rule, case).unwrap_or_else(|| panic!("{rule} {case}"));
                assert!(run.first().is_some(), "{rule} {case} has a block after the change");
                assert!(run.back.is_some() || run.watched.is_some(), "{rule} {case} came back, or says how long it was watched");
            }
        }
    }

    /// The numbers `docs/technical.md` tables for the same runs: Bitcoin's rule not back within
    /// two hours after a drop, and the blocks until the others were back.
    #[test]
    fn the_runs_read_as_the_measurement_tables_them() {
        let never = run("bitcoin", "drop20").expect("bitcoin drop20");
        assert_eq!((never.back, never.watched), (None, Some(7_200.0)));
        let backs = [("bitcoin", "rise50", 35), ("bip94", "drop20", 42), ("bip94", "drop50", 65), ("asert", "drop20", 25), ("asert", "drop50", 18)];
        for (rule, case, blocks) in backs {
            assert_eq!(run(rule, case).and_then(Run::blocks_to_back), Some(blocks), "{rule} {case}");
        }
        assert!(cases().iter().all(|case| earliest(case) <= 0.0));
    }

    #[test]
    fn a_block_before_the_change_counts_neither_first_nor_toward_back() {
        let block = |secs: f64| Block { secs, easy: secs < 0.0 };
        let blocks = vec![block(-40.0), block(-5.0), block(12.0), block(50.0)];
        let run = Run { rule: "asert".into(), case: "rise50".into(), target: 30.0, blocks, back: Some(50.0), watched: None, late_floor: Some(true) };
        assert_eq!(run.first(), Some(12.0));
        assert_eq!(run.blocks_to_back(), Some(2));
    }

    #[test]
    fn the_pace_is_back_where_gaps_stay_near_the_target() {
        let block = |secs: f64, easy: bool| Block { secs, easy };
        let mut blocks = vec![block(600.0, false), block(660.0, true), block(720.0, true)];
        blocks.extend((0..6).map(|n| block(800.0 + 30.0 * f64::from(n), false)));
        assert_eq!(back_by_pace(&blocks, 30.0), Some(800.0));
        assert_eq!(back_by_pace(&blocks[..4], 30.0), None);
        assert_eq!(case_words("drop20"), Some(("coin.case_drop", "20")));
        assert_eq!(case_words("rise50"), Some(("coin.case_rise", "50")));
        assert_eq!(case_words("leave"), None);
    }
}
