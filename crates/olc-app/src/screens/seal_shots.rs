//! Contacts and the wallet's password as a person meets them, in basic Korean and expert English,
//! kept as pictures too when `OLC_SEAL_SHOTS` names a folder. Everything in them is real: the
//! wallet mines its own coins, saves contacts, picks one on Send, locks itself with a password,
//! refuses a wrong one, opens with the right one, pays, and offers to save the address it paid.
use super::shots::{shot, tap};
use super::{app, harness, port_of_scene, quiet_settings, scratch, type_into_last, type_secret, wait_until, Window, SETTLE_STEPS, SIZES};
use crate::core::{coin_folder, save_coin, Command, ContactOrder, Personal, WalletOrder};
use crate::i18n::{self, Lang, Mode, Voice};
use crate::settings::{Paths, Theme};
use egui::accesskit::Role;
use egui_kittest::kittest::Queryable;
use olc_core::CoinConfig;
use olc_wallet::{format_address, Wallet};
use std::path::{Path, PathBuf};

const PASSWORD: &str = "tea at four";

struct Scene {
    lang: Lang,
    mode: Mode,
    port: u16,
    /// Three people already saved, and the one the person adds on the page.
    names: [&'static str; 4],
    /// What the person types on Send to find the first of them.
    typed: &'static str,
}

fn someone(seed: u8) -> String {
    format_address(&Wallet::from_secret([seed; 32]).unwrap().address())
}

fn say(window: &Window, key: &str) -> String {
    let settings = window.state().settings();
    i18n::text(Voice { lang: settings.language, mode: settings.mode }, key).to_string()
}

fn shows(window: &Window, text: &str) -> bool {
    window.query_all_by_label_contains(text).next().is_some()
}

fn picture(window: &mut Window, folder: Option<&Path>, name: &str) {
    window.run_steps(SETTLE_STEPS);
    if let Some(folder) = folder {
        shot(window, folder, name);
    }
}

/// The last control of this name: on a sheet over the page, the sheet's own.
fn tap_last(window: &mut Window, key: &str) {
    let name = say(window, key);
    window.query_all_by_label(&name).last().unwrap_or_else(|| panic!("{key} is not on the screen")).click_accesskit();
    window.run_steps(SETTLE_STEPS);
}

fn scroll_to(window: &mut Window, key: &str) {
    let name = say(window, key);
    if let Some(node) = window.query_all_by_label(&name).next() {
        node.scroll_to_me();
    }
    window.run_steps(SETTLE_STEPS);
}

fn close(window: &mut Window) {
    window.key_press(egui::Key::Escape);
    window.run_steps(SETTLE_STEPS);
}

fn start(scene: &Scene, name: &str) -> Window {
    let root = scratch(name);
    let paths = Paths { root: root.clone() };
    let mut coin = CoinConfig::olc();
    coin.name = "Shelf".into();
    coin.ticker = "SHF".into();
    coin.message = "Shelf contacts".into();
    coin.initial_difficulty = 30_000;
    coin.rules.block_time_ms = 2_000;
    save_coin(&paths, &coin).unwrap();
    let mut settings = quiet_settings();
    settings.coin = Some(coin_folder(&coin));
    settings.mine_on_start = true;
    settings.language = scene.lang;
    settings.mode = scene.mode;
    settings.theme = Theme::Dark;
    settings.port = port_of_scene(scene.port);
    let mut window = harness(app(root, settings, None), SIZES[0]);
    wait_until(&mut window, |app| app.snapshot().node.spendable > 2_000_000_000);
    window
}

fn contact(window: &Window, name: &str, address: String) {
    let order = WalletOrder::Contact(ContactOrder::Add { name: name.into(), address });
    window.state().send(Command::Own(Personal::Wallet(order)));
}

/// The Wallet page's contacts: three saved, a fourth added by hand, one renamed.
fn contacts(window: &mut Window, scene: &Scene, folder: Option<&Path>, tag: &str) {
    for (seed, name) in scene.names[..3].iter().enumerate() {
        contact(window, name, someone(seed as u8 + 11));
    }
    wait_until(window, |app| app.snapshot().node.wallet.contacts.len() == 3);
    window.state_mut().open("page.wallet");
    window.run_steps(SETTLE_STEPS);
    scroll_to(window, "contacts.add");
    tap(window, "contacts.add");
    type_into_last(window, Role::TextInput, 2, 0, scene.names[3]);
    type_into_last(window, Role::TextInput, 2, 1, &someone(14));
    tap(window, "common.save");
    wait_until(window, |app| app.snapshot().node.wallet.contacts.len() == 4);
    window.run_steps(SETTLE_STEPS);
    scroll_to(window, "contacts.add");
    picture(window, folder, &format!("contacts-{tag}"));
    assert!(scene.names.iter().all(|name| shows(window, name)), "every contact shows by name");
    // The same address under a second name is refused, and says whose it is.
    tap(window, "contacts.add");
    type_into_last(window, Role::TextInput, 2, 0, "Twice");
    type_into_last(window, Role::TextInput, 2, 1, &someone(14));
    tap(window, "common.save");
    wait_until(window, |app| app.snapshot().notice.as_ref().is_some_and(|notice| notice.key == "notice.contact_known"));
    window.run_steps(SETTLE_STEPS);
    assert!(shows(window, &say(window, "notice.contact_known").replace("{}", scene.names[3])), "the refusal names the contact");
    tap(window, "common.cancel");
    assert_eq!(window.state().snapshot().node.wallet.contacts.len(), 4);
}

/// Send finds a contact by part of its name, and picking it fills in its address.
fn send_picks(window: &mut Window, scene: &Scene, folder: Option<&Path>, tag: &str) {
    tap(window, "home.send");
    type_into_last(window, Role::TextInput, 3, 0, scene.typed);
    picture(window, folder, &format!("send-contact-{tag}"));
    let offered = window.query_all_by_label_contains(scene.names[0]).last().expect("the contact offered by name");
    offered.click_accesskit();
    window.run_steps(SETTLE_STEPS);
    let named = say(window, "contacts.to_named").replace("{}", scene.names[0]);
    assert!(shows(window, &named), "the field holds the contact's address and says whose");
    close(window);
}

/// The lock sheet sets a password; the wallet locks at once and keeps counting its coins.
fn lock(window: &mut Window, folder: Option<&Path>, tag: &str) {
    tap(window, "seal.set");
    type_secret(window, 0, PASSWORD);
    type_secret(window, 1, PASSWORD);
    picture(window, folder, &format!("lock-sheet-{tag}"));
    tap(window, "seal.go");
    wait_until(window, |app| app.snapshot().node.wallet.lock.sealed);
    window.run_steps(SETTLE_STEPS);
    picture(window, folder, &format!("lock-sheet-locked-{tag}"));
    assert!(shows(window, &say(window, "seal.ask")), "a locked wallet asks for its password in the sheet");
    tap_last(window, "explain.details");
    assert!(shows(window, &say(window, "seal.lost")), "Details says what a lost password leaves");
    close(window);
    assert!(window.state().snapshot().node.balance > 0, "a locked wallet still shows its balance");
    // The seed words ask for the password too; the first Open on the page is theirs.
    tap(window, "common.open");
    picture(window, folder, &format!("words-locked-{tag}"));
    assert!(shows(window, &say(window, "seal.ask")), "the seed words ask for the password first");
    close(window);
    // A backup made while locked stays locked with the wallet's password, and says so.
    tap(window, "settings.backup_go");
    picture(window, folder, &format!("backup-locked-{tag}"));
    assert!(shows(window, &say(window, "seal.backup_locked")), "the backup says it stays locked");
    close(window);
}

/// Send asks for the password first; a wrong one says so and opens nothing; the right one opens
/// the wallet, the payment goes, and Send offers to save the address it paid.
fn send_asks(window: &mut Window, folder: Option<&Path>, tag: &str) {
    tap(window, "home.send");
    let stranger = someone(40);
    type_into_last(window, Role::TextInput, 3, 0, &stranger);
    type_into_last(window, Role::TextInput, 3, 1, "2");
    type_secret(window, 0, "not it");
    picture(window, folder, &format!("send-password-{tag}"));
    tap_last(window, "seal.open");
    wait_until(window, |app| app.snapshot().node.wallet.lock.wrong == 1);
    window.run_steps(SETTLE_STEPS);
    assert!(shows(window, &say(window, "notice.wrong_wallet_password")), "a wrong password says so beside the field");
    assert!(window.state().snapshot().node.wallet.lock.open_for.is_none());
    type_secret(window, 0, PASSWORD);
    tap_last(window, "seal.open");
    wait_until(window, |app| app.snapshot().node.wallet.lock.open_for.is_some());
    window.run_steps(SETTLE_STEPS);
    tap_last(window, "send.next");
    tap_last(window, "send.sign");
    wait_until(window, |app| app.snapshot().notice.as_ref().is_some_and(|notice| notice.key == "notice.sent"));
    window.run_steps(SETTLE_STEPS);
    picture(window, folder, &format!("offer-{tag}"));
    assert!(shows(window, &say(window, "contacts.offer")), "a payment to a new address offers to save it");
    type_into_last(window, Role::TextInput, 1, 0, "Shop");
    tap_last(window, "common.save");
    wait_until(window, |app| app.snapshot().node.wallet.contacts.iter().any(|contact| contact.address == stranger));
}

fn run(scene: Scene, name: &str, tag: &str) {
    let pictures: Option<PathBuf> = std::env::var_os("OLC_SEAL_SHOTS").map(PathBuf::from);
    let folder = pictures.as_deref();
    let mut window = start(&scene, name);
    contacts(&mut window, &scene, folder, tag);
    send_picks(&mut window, &scene, folder, tag);
    lock(&mut window, folder, tag);
    send_asks(&mut window, folder, tag);
}

#[test]
fn contacts_and_the_password_in_plain_korean() {
    let scene = Scene { lang: Lang::Ko, mode: Mode::Basic, port: 52, names: ["민지", "단골 카페", "엄마", "회사 동료"], typed: "민" };
    run(scene, "seal-shots-ko", "ko-basic");
}

#[test]
fn contacts_and_the_password_in_expert_english() {
    let scene = Scene { lang: Lang::En, mode: Mode::Expert, port: 56, names: ["Mina", "Corner cafe", "Mom", "Office"], typed: "mi" };
    run(scene, "seal-shots-en", "en-expert");
}
