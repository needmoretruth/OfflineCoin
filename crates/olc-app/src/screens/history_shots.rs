//! The wallet's history while it mines, a row opened, a request with an amount, a payment to two
//! people, and the explorer's block and address, as a person meets them. Kept as pictures when
//! `OLC_HISTORY_SHOTS` names a folder. Everything is real: the app mines its own fast coin until
//! its wallet joins sixteen mined coins, then pays two people and another address of its own.
use super::shots::{shot, tap};
use super::{app, harness, port_of_scene, quiet_settings, scratch, type_into_last, wait_until, Window, SETTLE_STEPS, SIZES};
use crate::core::{coin_folder, save_coin, ActivityKind, Command, Explored, Personal, WalletOrder};
use crate::i18n::{self, Lang, Mode, Voice};
use crate::pages::history::HistoryList;
use crate::pages::{Cx, Page};
use crate::settings::{Paths, Theme};
use crate::ui::Icon;
use egui::accesskit::Role;
use egui_kittest::kittest::Queryable;
use olc_core::CoinConfig;
use std::path::Path;

const SOMEBODY: &str = "olc409c2c2374917964bd7d010ddb77f90500eb3a41";
const SOMEONE_ELSE: &str = "olc1b5f2c37c0a2fd43a6f9b39c1b0f9e2fbd0a7d1e";

/// The full history where the Wallet page will hold it.
#[derive(Default)]
struct HistoryHost {
    list: HistoryList,
}

impl Page for HistoryHost {
    fn title(&self) -> &'static str {
        "page.home"
    }

    fn icon(&self) -> Icon {
        Icon::Wallet
    }

    fn show(&mut self, ui: &mut egui::Ui, cx: &mut Cx) {
        crate::ui::widgets::title(ui, cx.say(HistoryList::TITLE));
        self.list.full(ui, cx);
    }
}

fn word(window: &Window, key: &str) -> String {
    let settings = window.state().settings();
    i18n::text(Voice { lang: settings.language, mode: settings.mode }, key).to_string()
}

fn picture(window: &mut Window, folder: Option<&Path>, name: &str) {
    window.run_steps(SETTLE_STEPS);
    if let Some(folder) = folder {
        shot(window, folder, name);
    }
}

fn shows(window: &Window, text: &str) -> bool {
    window.query_all_by_label_contains(text).next().is_some()
}

fn close(window: &mut Window) {
    window.key_press(egui::Key::Escape);
    window.run_steps(SETTLE_STEPS);
}

fn speak(window: &mut Window, lang: Lang, mode: Mode) {
    let settings = window.state_mut().settings_mut();
    settings.language = lang;
    settings.mode = mode;
    window.run_steps(SETTLE_STEPS);
}

fn wallet(window: &Window, order: WalletOrder) {
    window.state().send(Command::Own(Personal::Wallet(order)));
}

/// Waits in pieces, as the slowest mining computer needs more than one minute.
#[track_caller]
fn wait_long(window: &mut Window, minutes: u64, mut done: impl FnMut(&crate::app::App) -> bool) {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(minutes * 60);
    while std::time::Instant::now() < until {
        window.run_steps(1);
        if done(window.state()) {
            // The state can arrive after the frame was drawn: draw it before anyone looks.
            window.run_steps(3);
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let node = &window.state().snapshot().node;
    panic!("waited {minutes} min: height {}, rows {:?}", node.height, node.wallet.history.iter().map(|item| item.kind).collect::<Vec<_>>());
}

fn waiting_join(item: &crate::core::Activity) -> bool {
    item.kind == ActivityKind::Joined && item.height.is_none()
}

fn has_row(app: &crate::app::App, kind: ActivityKind) -> bool {
    app.snapshot().node.wallet.history.iter().any(|item| item.kind == kind)
}

/// A request with an amount and a note, then a payment to two people signed on its slip.
fn receive_and_send(window: &mut Window, folder: Option<&Path>, suffix: &str, sign: bool) {
    window.state_mut().open("page.home");
    window.run_steps(SETTLE_STEPS);
    tap(window, "home.receive");
    type_into_last(window, Role::TextInput, 2, 0, "12.5");
    type_into_last(window, Role::TextInput, 2, 1, "Lunch");
    picture(window, folder, &format!("receive-amount{suffix}"));
    assert!(shows(window, &word(window, "receive.copy_request")), "a request with an amount copies as a request");
    if window.state().settings().mode == Mode::Expert {
        assert!(shows(window, "amount=12.5&message=Lunch"), "an expert reads the request the code holds");
    }
    close(window);
    tap(window, "home.send");
    tap(window, "send.add_person");
    type_into_last(window, Role::TextInput, 5, 0, SOMEBODY);
    type_into_last(window, Role::TextInput, 5, 1, "3");
    type_into_last(window, Role::TextInput, 5, 2, SOMEONE_ELSE);
    type_into_last(window, Role::TextInput, 5, 3, "2");
    type_into_last(window, Role::TextInput, 5, 4, "Lunch");
    picture(window, folder, &format!("send-two{suffix}"));
    tap(window, "send.next");
    window.run_steps(SETTLE_STEPS);
    let to = word(window, "send.to");
    assert_eq!(window.query_all_by_label(&to).count(), 2, "the slip names both people");
    picture(window, folder, &format!("send-slip{suffix}"));
    if sign {
        // The slip's button shares its word with Home's Send behind it; the slip is drawn last.
        let sign = word(window, "send.sign");
        window.query_all_by_label(&sign).last().expect("the slip's sign button").click_accesskit();
        window.run_steps(2);
        let _ = super::wait_for_notice(window, "notice.sent");
        // Both people are new, so Send offers to save them as contacts; this person says not now.
        window.run_steps(SETTLE_STEPS);
        let later = word(window, "contacts.not_now");
        window.query_all_by_label(&later).last().expect("Send offers to save the new addresses").click_accesskit();
        window.run_steps(SETTLE_STEPS);
    } else {
        tap(window, "common.back");
    }
}

#[test]
fn history_rows_detail_requests_two_people_and_the_explorer_as_a_person_meets_them() {
    let pictures = std::env::var_os("OLC_HISTORY_SHOTS").map(std::path::PathBuf::from);
    let folder = pictures.as_deref();
    let root = scratch("history-shots");
    let paths = Paths { root: root.clone() };
    std::fs::create_dir_all(paths.wallet()).unwrap();
    olc_store::save_wallet(&paths.wallet(), &olc_wallet::Wallet::from_secret([7; 32]).unwrap()).unwrap();
    let mut coin = CoinConfig::olc();
    coin.name = "Ledger".into();
    coin.ticker = "LDG".into();
    coin.message = "Ledger words".into();
    coin.initial_difficulty = 10_000;
    coin.rules.block_time_ms = 1_000;
    coin.rules.coinbase_maturity = 2;
    coin.upgrade_from = 0;
    save_coin(&paths, &coin).unwrap();
    let mut settings = quiet_settings();
    settings.coin = Some(coin_folder(&coin));
    settings.mine_on_start = true;
    settings.language = Lang::Ko;
    settings.theme = Theme::Dark;
    settings.port = port_of_scene(74);
    let mut window = harness(app(root, settings, None), SIZES[0]);
    // A computer merges small coins only after its person turns the switch in Settings on.
    window.state().send(Command::Own(Personal::SetMerging(true)));

    // Sixteen ripe mined coins make the wallet join eight of them into one.
    wait_long(&mut window, 4, |app| has_row(app, ActivityKind::Joined));
    receive_and_send(&mut window, folder, "", true);
    speak(&mut window, Lang::En, Mode::Expert);
    receive_and_send(&mut window, folder, "-expert", false);
    speak(&mut window, Lang::Ko, Mode::Basic);

    // A payment to another address of this wallet is not a join.
    let first = window.state().snapshot().node.address.clone();
    wallet(&window, WalletOrder::NewAddress);
    let _ = super::wait_for_notice(&mut window, "notice.new_address");
    wallet(&window, WalletOrder::Pay { to: vec![(first, "1".into())], rate: 0, coins: Vec::new(), label: String::new() });
    wait_long(&mut window, 2, |app| {
        let snap = app.snapshot();
        let mined = |kind| snap.node.wallet.history.iter().any(|item| item.kind == kind && item.height.is_some());
        mined(ActivityKind::Sent) && mined(ActivityKind::ToSelf)
    });

    // The full list: when, what, how sure and how much, counting up as blocks come.
    window.state_mut().put_page(Box::<HistoryHost>::default());
    picture(&mut window, folder, "history-mining");
    for text in ["코인 8개(", "LDG)를 하나로 합침", "채굴", "보냄", "내 다른 주소로 1 LDG", "초 전", "Lunch"] {
        assert!(shows(&window, text), "the history shows {text}");
    }
    assert!(!shows(&window, "나에게"), "a join is not called a payment to yourself");
    let ripening = word(&window, "history.ripe_in").replace("{}", "");
    assert!(shows(&window, ripening.trim()), "a fresh mined row says when its coins can be spent");

    speak(&mut window, Lang::En, Mode::Expert);
    picture(&mut window, folder, "history-mining-expert");
    assert!(shows(&window, "Confirmations"), "an expert reads the number of confirmations");

    // Merging stops first and mining runs on until every join is in a block: a join made after
    // the last block would wait forever, and its row, the newest, has no block to open.
    window.state().send(Command::Own(Personal::SetMerging(false)));
    wait_long(&mut window, 1, |app| !crate::pages::history::every_row(&app.snapshot().node).any(waiting_join));
    // Rows stop moving under the pointer once mining stops, so a click lands on the joined row.
    window.state().send(Command::SetMining(false));
    let (mut last, mut same) = (0, 0);
    wait_long(&mut window, 1, |app| {
        let node = &app.snapshot().node;
        same = if node.height == last && !node.mining { same + 1 } else { 0 };
        last = node.height;
        same > 30
    });
    opened(&mut window, folder, "-expert", "LDG) joined into one", "Inputs (8)");
    assert!(shows(&window, "Mempool"), "an expert sees the payments waiting");
    speak(&mut window, Lang::Ko, Mode::Basic);
    opened(&mut window, folder, "", "LDG)를 하나로 합침", "쓴 코인 (8)");
}

/// A joined row opens into its coins, eight in and one out, all of them this wallet's; its
/// block opens from there, and then this wallet's first address.
fn opened(window: &mut Window, folder: Option<&Path>, suffix: &str, row: &str, inputs: &str) {
    window.state_mut().open("page.home");
    window.run_steps(SETTLE_STEPS);
    let joined = window.query_all_by_label_contains(row).next().expect("a joined row");
    joined.click();
    window.run_steps(SETTLE_STEPS);
    wait_until(window, |app| matches!(&app.snapshot().node.explored, Explored::Block(block) if block.txs.len() > 1));
    picture(window, folder, &format!("history-detail{suffix}"));
    assert!(shows(window, inputs), "the detail lists the eight coins it spent");
    tap(window, "detail.open_block");
    window.run_steps(SETTLE_STEPS);
    picture(window, folder, &format!("block-detail{suffix}"));
    for key in ["records.miner", "records.fees", "records.go_previous", "detail.size"] {
        assert!(shows(window, &word(window, key)), "the block shows {key}");
    }
    let mine = window.state().snapshot().node.keys[0].address.clone();
    window.state().send(Command::Own(Personal::Explore(mine)));
    wait_until(window, |app| matches!(&app.snapshot().node.explored, Explored::Address { moves, .. } if !moves.is_empty()));
    picture(window, folder, &format!("address-history{suffix}"));
    assert!(shows(window, &word(window, "records.address_moves")), "an address lists its payments");
}
