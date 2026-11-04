//! Who found the latest blocks, as the Mining page names them: this computer, its added miners,
//! a board, its own pool and finders known only by a long address. An address stands whole in its
//! short form, with one mark in the middle, at normal size and at 150 % text in a small window,
//! and the row still says the whole address to a hover or a screen reader.
//! OLC_SHOTS=<folder> saves the page at 1280 by 800 and at 960 by 620 with 150 % text.
use super::shots::shot;
use super::{app, harness, quiet_settings, scratch, staged, Window, SETTLE_STEPS, SIZES};
use crate::core::{Average, MinedBlock, Miner, Reading, Share};
use crate::i18n::{Lang, Mode};
use crate::settings::Theme;
use egui_kittest::kittest::{By, NodeT, Queryable};
use olc_core::CoinConfig;

const AWAY: &str = "olc4ab6b30f0e9d2c7a1b5e8f3d6c9a2b4e7f0c3d6a9b2e5f8c1d4a7b0e3f6a9c2";
const POOL: &str = "olc9f3e1d7c5b3a1f9e7d5c3b1a9f7e5d3c1b9a7f5e3d1c9b7a5f3e1d9c7b5a3";

/// Sixty blocks two seconds apart, found by six miners as the breakdown counts them.
fn stage(window: &mut Window) {
    let mut stage = staged(Vec::new());
    stage.node.coin = Some(CoinConfig::olc());
    stage.node.mining = true;
    stage.node.height = 1_060;
    stage.node.hashrate = 52_000;
    let now = u64::try_from(super::live_swap::unix_ms()).unwrap_or(u64::MAX);
    let finders = [
        (Miner::You, 20_u32, Some(52_000), Vec::new()),
        (Miner::Here("miner-a".into()), 12, Some(31_000), Vec::new()),
        (Miner::Away(AWAY.into()), 9, None, Vec::new()),
        (Miner::Board("보드원".into()), 8, Some(4_000), Vec::new()),
        (Miner::OwnPool, 6, Some(18_000), vec![Miner::Here("miner-b".into())]),
        (Miner::Pool(POOL.into()), 5, None, Vec::new()),
    ];
    let chain = &mut stage.node.chain;
    let order: Vec<&Miner> = finders.iter().flat_map(|(miner, blocks, _, _)| std::iter::repeat_n(miner, *blocks as usize)).collect();
    chain.latest = order
        .iter()
        .enumerate()
        .map(|(back, miner)| MinedBlock {
            height: 1_060 - back as u64,
            time_ms: now - 2_000 * back as u64,
            difficulty: 220_000,
            miner: (*miner).clone(),
            ..MinedBlock::default()
        })
        .collect();
    chain.shares = finders.into_iter().map(|(miner, blocks, hashrate, workers)| Share { miner, blocks, hashrate, workers }).collect();
    chain.hashrate_here = 105_000;
    chain.hashrate_all = 128_000;
    chain.network = Some(Reading { blocks: 20, span_ms: 40_000, rate: 128_000, others: 6, others_rate: 23_000 });
    chain.average = Some(Average { blocks: 59, span_ms: 118_000, rate: 112_000, measured: None });
    window.state_mut().set_stage(Some(stage));
    window.state_mut().open("page.mining");
    window.run_steps(SETTLE_STEPS);
}

fn rows(window: &Window) -> Vec<String> {
    let said = |node: egui_kittest::Node<'_>| {
        let node = node.accesskit_node();
        if node.role() == egui::accesskit::Role::Label { node.value() } else { node.label() }
    };
    window.query_all(By::new().label_contains("%, ").include_labels()).filter_map(said).collect()
}

#[test]
fn every_finder_is_named_whole_in_the_breakdown() {
    let mut settings = quiet_settings();
    settings.theme = Theme::Dark;
    settings.language = Lang::Ko;
    let mut window = harness(app(scratch("finders"), settings, None), SIZES[0]);
    let folder = std::env::var_os("OLC_SHOTS").map(std::path::PathBuf::from);
    for (mode, tag) in [(Mode::Expert, "ko-expert"), (Mode::Basic, "ko-basic")] {
        window.state_mut().settings_mut().mode = mode;
        stage(&mut window);
        if let Some(folder) = &folder {
            shot(&mut window, folder, &format!("finders-mining-{tag}-dark"));
        }
        let rows = rows(&window);
        let named = |start: &str| rows.iter().find(|row| row.starts_with(start)).cloned();
        for start in ["나, ", "miner-a, ", "보드원, ", "내 풀, ", "olc4ab6…a9c2, ", "풀 olc9f3e…b5a3, "] {
            assert!(named(start).is_some(), "{tag}: no row named {start:?} in {rows:?}");
        }
        // The short form leaves the middle out once; the whole address is still said.
        assert!(named("olc4ab6…a9c2, ").is_some_and(|row| row.ends_with(AWAY)), "{tag}: {rows:?}");
        assert!(named("풀 olc9f3e…b5a3, ").is_some_and(|row| row.contains("olc9f3e…b5a3, ") && row.ends_with(POOL)), "{tag}: {rows:?}");
        assert!(named("내 풀, ").is_some_and(|row| row.contains("miner-b")), "{tag}: the pool's row names its worker: {rows:?}");
        if let Some(folder) = &folder {
            // 960 by 620 pixels at 150 % text: the zoom lands first, then the size in its points,
            // tall enough to reach the breakdown.
            window.state_mut().settings_mut().text_percent = 150;
            window.run_steps(2);
            window.set_size(egui::vec2(960.0, 1_860.0) / 1.5);
            window.run_steps(SETTLE_STEPS);
            shot(&mut window, folder, &format!("finders-mining-{tag}-dark-960-150"));
            window.state_mut().settings_mut().text_percent = 100;
            window.run_steps(2);
            window.set_size(egui::vec2(SIZES[0].0 as f32, SIZES[0].1 as f32));
        }
    }
}
