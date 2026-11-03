//! The pacing card on Mining follows the coin's own rule on the chain: the wait for the next change
//! for a coin that changes every so many blocks, how the last blocks moved it for one that changes
//! at every block, and "never" for one that keeps it fixed.
use super::{app, harness, quiet_settings, scratch, staged, Window, SETTLE_STEPS, SIZES};
use crate::core::MinedBlock;
use crate::i18n::{Lang, Mode};
use egui_kittest::kittest::Queryable;
use olc_core::{CoinConfig, DifficultyRule};

/// The height of the staged tip: 7 blocks into a window of 20.
const HEIGHT: u64 = 47;

/// Mining on a coin under Bitcoin's rule that changes the difficulty every `every` blocks, after
/// twenty blocks that each took a little more work than the one before, 100 to 119. OLC's own
/// rule changes it at every block whatever the window, so it would never show a wait.
fn with_rule(window: &mut Window, every: u64) {
    let mut stage = staged(Vec::new());
    let mut coin = CoinConfig::olc();
    coin.rules.difficulty_rule = DifficultyRule::Bitcoin;
    coin.rules.retarget_interval = every;
    stage.node.coin = Some(coin);
    stage.node.height = HEIGHT;
    stage.node.difficulty = 119;
    stage.node.chain.latest =
        (0..20).map(|back| MinedBlock { height: HEIGHT - back, difficulty: 119 - back, ..MinedBlock::default() }).collect();
    window.state_mut().set_stage(Some(stage));
    window.state_mut().open("page.mining");
    window.run_steps(SETTLE_STEPS);
}

fn shows(window: &Window, text: &str) -> bool {
    window.query_all_by_label_contains(text).next().is_some()
}

#[test]
fn the_pacing_card_follows_the_coins_rule() {
    let mut window = harness(app(scratch("pacing-rule"), quiet_settings(), None), SIZES[0]);
    window.state_mut().settings_mut().language = Lang::En;
    window.state_mut().settings_mut().mode = Mode::Basic;

    with_rule(&mut window, 1);
    assert!(shows(&window, "Recent change") && shows(&window, "19% harder"), "a coin that changes at every block shows how the last blocks moved it");
    assert!(shows(&window, "Over the last 20 blocks"));
    assert!(!shows(&window, "Next change"), "it has no wait for a next change");
    if let Some(folder) = std::env::var_os("OLC_SHOTS").map(std::path::PathBuf::from) {
        window.state_mut().settings_mut().language = Lang::Ko;
        window.run_steps(SETTLE_STEPS);
        super::shots::shot(&mut window, &folder, "mining-each-block-basic-ko-dark");
        window.state_mut().settings_mut().language = Lang::En;
    }

    with_rule(&mut window, 20);
    assert!(shows(&window, "Next change") && shows(&window, "In 13 blocks"), "a coin that changes every 20 blocks shows the wait");
    assert!(!shows(&window, "Recent change"));

    with_rule(&mut window, 0);
    assert!(shows(&window, "Never"), "a coin that never changes says so");
    assert!(!shows(&window, "Recent change"));
}
