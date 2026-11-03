//! The wallet's seed words, a shared wallet and Speed up as a person meets them, kept as
//! pictures too when `OLC_WALLET_SHOTS` names a folder. Everything in them is real: a wallet from before seed words mines its own
//! coin, gets words, moves its coins to them, pays a shared wallet it made, starts a spend from
//! it, and stops mining so a payment stays waiting.
use super::shots::{shot, tap};
use super::{app, harness, port_of_scene, quiet_settings, scratch, type_into_last, wait_until, Window, SETTLE_STEPS, SIZES};
use egui::accesskit::Role;
use crate::core::{coin_folder, save_coin, Command, LockOrder, Personal, WalletOrder};
use crate::i18n::{self, Lang, Voice};
use crate::pages::seed_words::asked;
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
/// The rows of the settings page with an Open button, in order.
const WORDS_ROW: usize = 3;
const SHARED_ROW: usize = 4;

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

fn picture(window: &mut Window, folder: Option<&Path>, name: &str) {
    window.run_steps(SETTLE_STEPS);
    if let Some(folder) = folder {
        shot(window, folder, name);
    }
}

fn tall(window: &mut Window, folder: Option<&Path>, name: &str) {
    window.set_size(egui::vec2(SIZES[1].0 as f32, SIZES[1].1 as f32));
    picture(window, folder, name);
    window.set_size(egui::vec2(SIZES[0].0 as f32, SIZES[0].1 as f32));
}

fn shows(window: &Window, text: &str) -> bool {
    window.query_all_by_label_contains(text).next().is_some()
}

fn settings_row(window: &mut Window, row: usize) {
    window.state_mut().open("page.settings");
    window.run_steps(SETTLE_STEPS);
    open_nth(window, "common.open", row);
}

#[test]
fn words_shared_signers_and_speed_up_as_a_person_meets_them() {
    let pictures = std::env::var_os("OLC_WALLET_SHOTS").map(std::path::PathBuf::from);
    if let Some(folder) = &pictures {
        std::fs::create_dir_all(folder).unwrap();
    }
    let folder = pictures.as_deref();
    let root = scratch("wallet-shots");
    let paths = Paths { root: root.clone() };
    // A wallet from before seed words: one key, no words.
    std::fs::create_dir_all(paths.wallet()).unwrap();
    olc_store::save_wallet(&paths.wallet(),&olc_wallet::Wallet::from_secret([5; 32]).unwrap()).unwrap();
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
    wait_until(&mut window, |app| app.snapshot().node.spendable > 2_000_000_000);

    // Home tells the older wallet it has no words yet, and its button opens the words sheet.
    window.state_mut().open("page.home");
    window.run_steps(SETTLE_STEPS);
    picture(&mut window, folder, "home-no-words");
    assert!(shows(&window, "아직 복구 단어가 없습니다"), "the home page says the wallet has no words");
    tap(&mut window, "words.make");
    window.run_steps(SETTLE_STEPS);
    assert!(shows(&window, "이 지갑에는 아직 복구 단어가 없습니다"), "the home button opens the words sheet");
    close(&mut window);

    // The older wallet gets words, writes them down and types two back.
    settings_row(&mut window, WORDS_ROW);
    picture(&mut window, folder, "words-none");
    tap(&mut window, "words.make");
    wait_until(&mut window, |app| app.snapshot().node.wallet.words_to_write.is_some());
    window.run_steps(SETTLE_STEPS);
    picture(&mut window, folder, "words-show");
    tap(&mut window, "words.wrote");
    picture(&mut window, folder, "words-check");
    let words = window.state().snapshot().node.wallet.words_to_write.clone().unwrap().0;
    for (field, place) in asked(&words).into_iter().enumerate() {
        type_into_last(&mut window, Role::TextInput, 2, field, &words[place]);
    }
    tap(&mut window, "words.confirm");
    wait_until(&mut window, |app| app.snapshot().node.wallet.words_to_write.is_none());

    // Coins from before the words need the backup file until one payment moves them.
    window.state_mut().open("page.home");
    wait_until(&mut window, |app| app.snapshot().node.wallet.outside_words > 0);
    window.run_steps(SETTLE_STEPS);
    picture(&mut window, folder, "home-outside");
    assert!(shows(&window, "단어로 되살릴 수 없는 코인이"), "the home page says some coins need more than the words");
    assert!(!shows(&window, "아직 복구 단어가 없습니다"), "the no-words line goes once the words exist");
    tap(&mut window, "common.open");
    picture(&mut window, folder, "words-move");
    assert!(shows(&window, "백업 파일이 있어야 합니다"), "the sheet says those coins need the backup file");
    assert!(!shows(&window, "키"), "no word a person does not know");
    assert!(!shows(&window, &words[0]), "the words stay hidden until asked");
    tap(&mut window, "words.see_again");
    picture(&mut window, folder, "words-ask");
    assert!(!shows(&window, &words[0]), "asking first is not yet showing");
    tap(&mut window, "words.show_go");
    tall(&mut window, folder, "words-again");
    assert!(words.iter().all(|word| shows(&window, word)), "every word shows again");
    let before = window.state().snapshot().node.wallet.outside_words;
    tap(&mut window, "words.move");
    let moved = super::wait_for_notice(&mut window, "notice.moved_to_words");
    assert!(moved.ends_with(" SHF"), "the amount moved carries its unit: {moved}");
    // What the payment moves no longer counts as needing the backup file.
    wait_until(&mut window, |app| app.snapshot().node.wallet.outside_words < before);
    picture(&mut window, folder, "words-moved");
    close(&mut window);

    // A shared wallet of this wallet's key and two others, paid from what the app mined.
    let mine = window.state().snapshot().node.keys.iter().find(|key| key.receiving).map(|key| key.public.clone());
    let keys = std::iter::once(mine.unwrap()).chain(OTHERS.map(str::to_string)).collect();
    wallet(&window, WalletOrder::MakeShared { need: 2, keys });
    wait_until(&mut window, |app| !app.snapshot().node.wallet.shared.is_empty());
    let text = window.state().snapshot().node.wallet.shared[0].text.clone();
    // The figure counts the coins a payment picks from, the move's output among them.
    wait_until(&mut window, |app| app.snapshot().node.spendable > 2_000_000_000);
    window.state().send(Command::Lock(LockOrder::Pay { words: text, amount: "12".into() }));
    wait_until(&mut window, |app| app.snapshot().node.wallet.shared.first().is_some_and(|shared| shared.balance > 0));
    settings_row(&mut window, SHARED_ROW);
    tap(&mut window, "shared.receive");
    tall(&mut window, folder, "shared-wallet");
    close(&mut window);
    let (to, amount) = (SOMEBODY.to_string(), "3".to_string());
    window.state().send(Command::Lock(LockOrder::PayShared { wallet: 0, to, amount }));
    wait_until(&mut window, |app| app.snapshot().node.half_spend.is_some());
    // The spend that waits for a second signature comes first.
    settings_row(&mut window, SHARED_ROW);
    tall(&mut window, folder, "shared-signatures");
    assert!(shows(&window, "2번, 3번 서명자의 서명을 기다립니다."), "the signers still to sign, by their place");
    let second = window.state().snapshot().node.wallet.shared[0].signers[1].clone();
    assert!(!shows(&window, &second), "basic mode leaves their addresses out");
    close(&mut window);

    // A payment that waits, because nothing mines, can go again with a higher fee.
    window.state().send(Command::SetMining(false));
    wait_until(&mut window, |app| !app.snapshot().node.mining);
    window.state().send(Command::Pay { to: SOMEBODY.into(), amount: "1".into() });
    window.state_mut().open("page.home");
    wait_until(&mut window, |app| !app.snapshot().node.wallet.speed.is_empty());
    window.run_steps(SETTLE_STEPS);
    picture(&mut window, folder, "speed-up-row");
    tap(&mut window, "home.speed_up");
    picture(&mut window, folder, "speed-up-slip");
    let quote = window.state().snapshot().node.wallet.speed[0];
    assert!(quote.new > quote.now, "the new fee is higher");
    assert!(shows(&window, "지금 수수료") && shows(&window, "새 수수료"), "both fees show");
    assert!(shows(&window, "받는 사람과 금액은 그대로이고"), "the recipient and amount stay");
    assert!(!shows(&window, "거래"), "basic mode leaves the payment id out");
    tap(&mut window, "speed.sign");
    let sped = super::wait_for_notice(&mut window, "notice.sped_up");
    assert!(sped.ends_with(" SHF"), "the new fee carries its unit: {sped}");
    picture(&mut window, folder, "speed-up-done");
}
