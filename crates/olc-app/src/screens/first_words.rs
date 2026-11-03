//! The first run on another computer: seed words written down elsewhere bring the wallet back
//! before the coin is chosen. When `OLC_WALLET_SHOTS` names a folder, the screens are kept as
//! pictures too. Another OLC Core running on this computer is found by the coin question, so the
//! question sees no coins around here: the test must not join a node it did not start.
use super::shots::{shot, tap};
use super::{app, click, harness, port_of_scene, quiet_settings, scratch, type_into_last, wait_until, SETTLE_STEPS, SIZES};
use egui::accesskit::Role;
use olc_core::CoinConfig;
use crate::i18n::Lang;
use olc_wallet::seed::Mnemonic;
use olc_wallet::{format_address, Wallet};

/// Words from the BIP's own test list, standing for ones a person wrote on paper.
const WORDS: &str = "legal winner thank year wave sausage worth useful legal winner thank yellow";

#[test]
fn words_bring_a_wallet_back_at_the_first_run() {
    let pictures = std::env::var_os("OLC_WALLET_SHOTS").map(std::path::PathBuf::from);
    let root = scratch("first-words");
    let mut settings = quiet_settings();
    settings.setup_done = false;
    settings.language = Lang::Ko;
    settings.port = port_of_scene(79);
    let mut window = harness(app(root, settings, None), SIZES[0]);
    window.run_steps(SETTLE_STEPS);

    tap(&mut window, "setup.have_words");
    type_into_last(&mut window, Role::MultilineTextInput, 1, 0, WORDS);
    if let Some(folder) = &pictures {
        shot(&mut window, folder, "first-run-words");
    }
    tap(&mut window, "words.restore_go");
    let expected = Wallet::from_words(&Mnemonic::parse(WORDS).unwrap(), true).unwrap().address();
    let expected = format_address(&expected);
    wait_until(&mut window, |app| app.snapshot().notice.as_ref().is_some_and(|notice| notice.key == "notice.words_restored"));
    window.run_steps(SETTLE_STEPS);
    if let Some(folder) = &pictures {
        shot(&mut window, folder, "first-run-restored");
    }
    // The setup goes on from its first question, and the node it starts has the words' wallet.
    let mut alone = (*window.state().snapshot()).clone();
    alone.groups.clear();
    window.state_mut().set_stage(Some(alone));
    tap(&mut window, "common.next");
    click(&mut window, &CoinConfig::olc().name);
    tap(&mut window, "setup.mine_later");
    window.state_mut().set_stage(None);
    wait_until(&mut window, |app| app.snapshot().node.address == expected);
    assert!(window.state().snapshot().node.wallet.words_to_write.is_none(), "the words are written down already");
}
