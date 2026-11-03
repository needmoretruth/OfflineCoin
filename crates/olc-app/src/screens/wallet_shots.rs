//! Pictures of the wallet's seed words, a shared wallet and Speed up, when `OLC_WALLET_SHOTS`
//! names a folder. Everything in them is real: the app mines its own coin, pays a shared wallet
//! it made, starts a spend from it, and stops mining so a payment stays waiting.
use super::shots::{shot, tap};
use super::{app, harness, port_of_scene, quiet_settings, scratch, wait_until, Window, SETTLE_STEPS, SIZES};
use crate::core::{coin_folder, save_coin, Command, LockOrder, Personal, WalletOrder};
use crate::i18n::{self, Lang, Voice};
use crate::settings::{Paths, Theme};
use egui_kittest::kittest::Queryable;
use olc_core::CoinConfig;
use std::path::Path;

/// Two points of the curve anyone can name, standing for the other two people.
const OTHERS: [&str; 2] = [
    "0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798",
    "02c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5",
];
const SOMEBODY: &str = "olc409c2c2374917964bd7d010ddb77f90500eb3a41";

fn open_nth(window: &mut Window, key: &str, index: usize) {
    let settings = window.state().settings();
    let name = i18n::text(Voice { lang: settings.language, mode: settings.mode }, key);
    let found = window.query_all_by_label(name).nth(index);
    found.expect("the control").click_accesskit();
    window.run_steps(SETTLE_STEPS);
}

fn close(window: &mut Window) {
    window.key_press(egui::Key::Escape);
    window.run_steps(SETTLE_STEPS);
}

fn wallet(window: &Window, order: WalletOrder) {
    window.state().send(Command::Own(Personal::Wallet(order)));
}

#[test]
fn wallet_screenshots() {
    let Some(folder) = std::env::var_os("OLC_WALLET_SHOTS").map(std::path::PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&folder).unwrap();
    let folder: &Path = &folder;
    let root = scratch("wallet-shots");
    let paths = Paths { root: root.clone() };
    let mut coin = CoinConfig::olc();
    coin.name = "Shelf".into();
    coin.ticker = "SHF".into();
    coin.message = "Shelf words".into();
    coin.initial_difficulty = 30_000;
    coin.rules.block_time_ms = 2_000;
    // A coin made in this app takes the new rules from its first block, as shared wallets need.
    coin.upgrade_from = 0;
    save_coin(&paths, &coin).unwrap();
    let mut settings = quiet_settings();
    settings.coin = Some(coin_folder(&coin));
    settings.mine_on_start = true;
    settings.language = Lang::Ko;
    settings.theme = Theme::Dark;
    settings.port = port_of_scene(78);
    let mut window = harness(app(root, settings, None), SIZES[0]);
    wait_until(&mut window, |app| app.snapshot().node.wallet.words_to_write.is_some());

    // A new wallet comes with its words, and the home page asks for them.
    window.state_mut().open("page.home");
    window.run_steps(SETTLE_STEPS);
    tap(&mut window, "home.words_write");
    shot(&mut window, folder, "words-show");
    tap(&mut window, "words.wrote");
    shot(&mut window, folder, "words-check");
    close(&mut window);

    // A shared wallet of this wallet's key and two others, paid from what the app mined.
    wait_until(&mut window, |app| app.snapshot().node.spendable > 2_000_000_000);
    let mine = window.state().snapshot().node.keys.iter().find(|key| key.receiving).map(|key| key.public.clone());
    let keys = std::iter::once(mine.unwrap()).chain(OTHERS.map(str::to_string)).collect();
    wallet(&window, WalletOrder::MakeShared { need: 2, keys });
    wait_until(&mut window, |app| !app.snapshot().node.wallet.shared.is_empty());
    let text = window.state().snapshot().node.wallet.shared[0].text.clone();
    window.state().send(Command::Lock(LockOrder::Pay { words: text, amount: "12".into() }));
    wait_until(&mut window, |app| app.snapshot().node.wallet.shared.first().is_some_and(|shared| shared.balance > 0));
    let (to, amount) = (SOMEBODY.to_string(), "3".to_string());
    window.state().send(Command::Lock(LockOrder::PayShared { wallet: 0, to, amount }));
    wait_until(&mut window, |app| app.snapshot().node.half_spend.is_some());
    window.state_mut().open("page.settings");
    window.run_steps(SETTLE_STEPS);
    open_nth(&mut window, "common.open", 4);
    tap(&mut window, "shared.receive");
    shot(&mut window, folder, "shared-wallet");
    // The pay form, and below it the spend that waits for a second signature.
    tap(&mut window, "shared.pay");
    window.set_size(egui::vec2(SIZES[1].0 as f32, SIZES[1].1 as f32));
    shot(&mut window, folder, "shared-signatures");
    window.set_size(egui::vec2(SIZES[0].0 as f32, SIZES[0].1 as f32));
    close(&mut window);

    // A payment that waits, because nothing mines, can go again with a higher fee.
    window.state().send(Command::SetMining(false));
    wait_until(&mut window, |app| !app.snapshot().node.mining);
    window.state().send(Command::Pay { to: SOMEBODY.into(), amount: "1".into() });
    window.state_mut().open("page.home");
    wait_until(&mut window, |app| app.snapshot().node.activity.iter().any(|item| item.height.is_none()));
    window.run_steps(SETTLE_STEPS);
    shot(&mut window, folder, "speed-up-row");
    tap(&mut window, "home.speed_up");
    shot(&mut window, folder, "speed-up-slip");
    tap(&mut window, "speed.sign");
    super::wait_for_notice(&mut window, "notice.sped_up");
    shot(&mut window, folder, "speed-up-done");
}
