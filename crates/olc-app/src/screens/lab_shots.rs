//! The Lab in the real app: every kind is one click from its first view in both modes, and so is
//! every setup. The tests that start real node processes wait for `--ignored`.
use super::{app, harness, quiet_settings, scratch, SETTLE_STEPS};
use crate::i18n::{self, Lang, Mode, Voice};
use crate::local::lab_net::{Kind, Scene};
use crate::pages::lab_palette::scene_key;
use crate::pages::lab_tokens::kind_key;
use egui_kittest::kittest::Queryable;

/// Every "+ kind" and every setup is on the Lab's first view, in both modes and languages.
#[test]
fn every_kind_and_setup_is_one_click_from_the_first_view() {
    let mut window = harness(app(scratch("lab_page"), quiet_settings(), None), (1280, 800));
    for mode in [Mode::Basic, Mode::Expert] {
        for lang in [Lang::En, Lang::Ko] {
            window.state_mut().settings_mut().mode = mode;
            window.state_mut().settings_mut().language = lang;
            window.state_mut().open("page.lab");
            window.run_steps(SETTLE_STEPS);
            let voice = Voice { lang, mode };
            for kind in Kind::ALL {
                let label = format!("+ {}", i18n::text(voice, kind_key(kind)));
                assert!(window.query_by_label(&label).is_some(), "no {label} in {mode:?} {lang:?}");
            }
            for scene in Scene::ALL {
                let label = i18n::text(voice, scene_key(scene));
                assert!(window.query_by_label(label).is_some(), "no {label} in {mode:?} {lang:?}");
            }
        }
    }
}
