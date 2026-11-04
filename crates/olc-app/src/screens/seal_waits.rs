//! What waits while the wallet is locked, said where a person looks: the pool's payouts on the
//! running pool's card, and merging small coins on the Wallet page's 「When sending」 card. With
//! `OLC_SEAL_SHOTS` it keeps a picture of each in plain Korean and in expert English.
use super::shots::shot;
use super::{app, harness, quiet_settings, scratch, Window, SETTLE_STEPS};
use crate::core::{BlockState, FoundRow, LockView, Snapshot, WorkerRow};
use crate::i18n::{self, Lang, Mode, Voice};
use crate::settings::{Settings, Theme};
use egui_kittest::kittest::Queryable;
use olc_core::coin::COIN;
use std::path::Path;

fn word(window: &Window, key: &str) -> &'static str {
    let settings = window.state().settings();
    i18n::text(Voice { lang: settings.language, mode: settings.mode }, key)
}

fn shows(window: &Window, key: &str) -> bool {
    window.query_all_by_label(word(window, key)).next().is_some()
}

fn picture(window: &mut Window, name: &str) {
    window.run_steps(SETTLE_STEPS);
    if let Some(folder) = std::env::var_os("OLC_SEAL_SHOTS") {
        shot(window, Path::new(&folder), name);
    }
}

fn scroll_to(window: &mut Window, key: &str) {
    if let Some(node) = window.query_all_by_label(word(window, key)).next() {
        node.scroll_to_me();
    }
    window.run_steps(SETTLE_STEPS);
}

/// A locked wallet that merges small coins, running a pool whose ripe block waits to pay a worker.
fn locked_pool(live: &Snapshot) -> Snapshot {
    let mut stage = live.clone();
    stage.node.wallet.lock = LockView { sealed: true, open_for: None, wrong: 0 };
    stage.node.merging = true;
    stage.pool.running = true;
    stage.pool.port = 3333;
    stage.pool.reach = vec!["127.0.0.1:3333".into()];
    let worker = WorkerRow {
        name: "olc1example.Kitchen".into(),
        worker: "Kitchen".into(),
        speed: 36_000,
        last_share_secs: Some(3),
        accepted: 1_080,
        online: true,
        ..WorkerRow::default()
    };
    stage.pool.workers = vec![worker];
    let block = |height, held, secs_ago| FoundRow { height, value: 50 * COIN, state: BlockState::Waiting, pool_fee: 0, secs_ago, held, parts: Vec::new() };
    // The newer block is not spendable yet; the older one is, and only the wallet holds it back.
    stage.pool.found = vec![block(47, false, 40), block(41, true, 640)];
    stage.pool.waits_for_wallet = true;
    stage
}

#[test]
fn what_waits_for_a_locked_wallet_says_so() {
    for (lang, mode, tag) in [(Lang::Ko, Mode::Basic, "ko-basic"), (Lang::En, Mode::Expert, "en-expert")] {
        let settings = Settings { language: lang, mode, theme: Theme::Dark, ..quiet_settings() };
        let mut window = harness(app(scratch(&format!("seal-waits-{tag}")), settings, None), (1280, 800));
        window.run_steps(SETTLE_STEPS);
        let stage = locked_pool(&window.state().snapshot());
        window.state_mut().set_stage(Some(stage));
        window.state_mut().open("page.mining");
        window.run_steps(SETTLE_STEPS);
        // The blocks' states are painted tags, so the picture shows the held one beside the other.
        scroll_to(&mut window, "pool.stop");
        assert!(shows(&window, "notice.pool_waits_wallet"), "{tag}: the pool's card says its payouts wait");
        picture(&mut window, &format!("pool-waits-{tag}"));
        window.state_mut().open("page.wallet");
        window.run_steps(SETTLE_STEPS);
        scroll_to(&mut window, "wallet_page.paying");
        assert!(shows(&window, "wallet_page.merge_waits"), "{tag}: the sending card says merging waits");
        picture(&mut window, &format!("merge-waits-{tag}"));
    }
}
