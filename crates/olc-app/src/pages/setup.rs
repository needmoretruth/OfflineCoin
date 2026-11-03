//! The first run: which coin, then whether to mine now and in which words, then a working wallet.
//! It does not ask for a device: this computer mines, and boards are added later from Devices.
//! Someone who already has seed words brings their wallet back from the first question, as
//! Electrum and Sparrow offer when a wallet is made.
use super::coin_new::CoinForm;
use super::Cx;
use crate::core::{Command, Personal, StartCoin, WalletOrder};
use crate::discovery::{CoinGroup, Reach};
use crate::i18n::Mode;
use crate::ui::explain::{self, Choice};
use crate::ui::format;
use crate::ui::paint::{surface, Surface};
use crate::ui::tokens;
use crate::ui::widgets::{self, button, button_on, meta, section, segments, words_box, Kind};
use egui::{pos2, vec2, Sense, Ui};
use olc_core::CoinConfig;

const WIDTH: f32 = 620.0;
const CHOICE_HEIGHT: f32 = 62.0;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Step {
    #[default]
    Coin,
    NewCoin,
    Mining,
    Words,
}

#[derive(Default)]
pub struct Setup {
    step: Step,
    form: CoinForm,
    start: Option<StartCoin>,
    words: String,
    /// The notice number when the words went, so the answer to them can be told apart.
    restoring: Option<u64>,
}

/// A wide choice with a line under its name.
fn choice(ui: &mut Ui, cx: &Cx, name: &str, about: &str) -> bool {
    let t = cx.t;
    let size = vec2(ui.available_width(), CHOICE_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hover = response.hovered();
    let look = Surface {
        top: if hover { t.hover } else { t.card_top },
        bottom: if hover { t.hover } else { t.card_bottom },
        line: t.line,
        glint: t.glint,
        shadow: None,
        radius: tokens::RADIUS_CARD,
    };
    surface(ui.painter(), rect, &look, None);
    let left = rect.left() + f32::from(tokens::CARD_MARGIN);
    let name_font = egui::FontId::new(tokens::TEXT, crate::ui::fonts::strong());
    ui.painter().text(pos2(left, rect.top() + 13.0), egui::Align2::LEFT_TOP, name, name_font, t.text);
    // The line is a label, so a screen reader reads what was found along with the name.
    let line = egui::Rect::from_min_max(pos2(left, rect.top() + 34.0), pos2(rect.right() - left + rect.left(), rect.bottom() - 6.0));
    let words = egui::RichText::new(about).size(tokens::META).color(t.muted);
    let place = egui::UiBuilder::new().max_rect(line).layout(egui::Layout::left_to_right(egui::Align::TOP));
    ui.scope_builder(place, |ui| ui.add(egui::Label::new(words).selectable(false).truncate()));
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(1.0),
            tokens::RADIUS_CARD,
            egui::Stroke::new(1.5, t.info),
            egui::StrokeKind::Outside,
        );
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
    response.clicked()
}

/// A coin found around, how many run it and the address to reach one by.
type Found = (CoinConfig, usize, Option<String>);

/// The coins found around. OfflineCoin comes apart from the others, since it has its own row.
fn found_coins(cx: &Cx) -> (Option<Found>, Vec<Found>) {
    let olc = CoinConfig::olc().genesis().id();
    let named = |group: &&CoinGroup| group.identity.is_some();
    let found = cx.snap.groups.iter().filter(named).filter_map(|group| {
        let entries = group.chains.iter().flat_map(|chain| &chain.entries);
        let around: Vec<_> = entries.filter(|entry| entry.reach != Reach::ThisApp).collect();
        let first = around.first()?;
        let identity = group.identity.as_ref()?;
        Some((identity.genesis == olc, (identity.coin.clone(), around.len(), first.address.clone())))
    });
    let (mut olc_found, mut rest) = (None, Vec::new());
    for (is_olc, coin) in found {
        if is_olc {
            olc_found = Some(coin);
        } else {
            rest.push(coin);
        }
    }
    (olc_found, rest)
}

impl Setup {
    /// Words from another computer. The wallet they make is this app's from then on; the
    /// coins show once the blocks arrive.
    fn words_step(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        widgets::title(ui, cx.say("words.restore"));
        meta(ui, cx.say("setup.words_body"), t);
        ui.add_space(tokens::GAP);
        let answer = cx.snap.notice.as_ref().filter(|notice| self.restoring.is_some_and(|sent| notice.number > sent));
        if let Some(notice) = answer.filter(|notice| notice.key == "notice.words_restored") {
            meta(ui, &cx.fill("setup.words_done", &[&notice.detail]), t);
            ui.add_space(tokens::GAP);
            if button(ui, t, cx.say("common.next"), Kind::Main).clicked() {
                self.step = Step::Coin;
            }
            return;
        }
        words_box(ui, &mut self.words, cx.say("words.restore_hint"), ui.available_width());
        ui.add_space(tokens::GAP);
        ui.horizontal(|ui| {
            let filled = self.words.split_whitespace().count() >= 12;
            if button_on(ui, t, cx.say("words.restore_go"), Kind::Main, filled).clicked() {
                self.restoring = Some(cx.snap.notice.as_ref().map_or(0, |notice| notice.number));
                let words = std::mem::take(&mut self.words);
                cx.send(Command::Own(Personal::Wallet(WalletOrder::RestoreWords(words))));
            }
            if button(ui, t, cx.say("common.back"), Kind::Quiet).clicked() {
                self.step = Step::Coin;
            }
        });
    }

    fn coin_step(&mut self, ui: &mut Ui, cx: &mut Cx) {
        widgets::title(ui, cx.say("setup.coin"));
        let (olc_found, others) = found_coins(cx);
        let olc = CoinConfig::olc();
        // OfflineCoin shows once: found around, its own row says so and joins them.
        let about = match &olc_found {
            Some((_, count, _)) => cx.fill("setup.olc_found", &[&count.to_string()]),
            None => cx.say("setup.olc_about").to_string(),
        };
        if choice(ui, cx, &olc.name, &about) {
            self.start = Some(match olc_found {
                Some((coin, _, address)) => StartCoin::Join { coin, address },
                None => StartCoin::Default,
            });
            self.step = Step::Mining;
        }
        if choice(ui, cx, cx.say("coins.new"), cx.say("setup.new_coin_about")) {
            self.step = Step::NewCoin;
        }
        // A coin can be found at any moment, so it goes under the choices a person may be pressing.
        for (coin, count, address) in others {
            let about = cx.fill("setup.found_about", &[&coin.ticker, &count.to_string()]);
            if choice(ui, cx, &coin.name, &about) {
                self.start = Some(StartCoin::Join { coin, address });
                self.step = Step::Mining;
            }
        }
        ui.add_space(tokens::GAP_WIDE);
        if choice(ui, cx, cx.say("setup.have_words"), cx.say("setup.have_words_about")) {
            self.step = Step::Words;
        }
    }

    fn new_coin_step(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        widgets::title(ui, cx.say("coins.new"));
        ui.scope(|ui| self.form.show(ui, cx));
        self.form.problem(ui, cx);
        ui.add_space(tokens::GAP_WIDE);
        let coin = self.form.coin(cx);
        ui.horizontal(|ui| {
            let next = button_on(ui, t, cx.say("common.next"), Kind::Main, coin.is_some());
            if let Some(coin) = coin.filter(|_| next.clicked()) {
                self.start = Some(StartCoin::New(coin));
                self.step = Step::Mining;
            }
            if button(ui, t, cx.say("common.back"), Kind::Quiet).clicked() {
                self.step = Step::Coin;
            }
        });
    }

    fn finish(&mut self, cx: &mut Cx) {
        cx.settings.setup_done = true;
        let start = self.start.take().unwrap_or(StartCoin::Default);
        cx.send(Command::FinishSetup(Box::new(cx.settings.clone()), start));
    }

    fn mining_step(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        widgets::title(ui, cx.say("setup.mine"));
        mode_choice(ui, cx);
        ui.add_space(tokens::GAP_WIDE);
        let about = cx.fill("setup.mine_about", &[&format::rate(cx.voice(), cx.settings.hash_cap)]);
        if choice(ui, cx, cx.say("setup.mine_now"), &about) {
            cx.settings.mine_on_start = true;
            self.finish(cx);
        }
        if choice(ui, cx, cx.say("setup.mine_later"), cx.say("setup.mine_later_about")) {
            cx.settings.mine_on_start = false;
            self.finish(cx);
        }
        ui.add_space(tokens::GAP);
        if button(ui, t, cx.say("common.back"), Kind::Quiet).clicked() {
            self.step = Step::Coin;
        }
    }

    pub fn show(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let margin = ((ui.available_width() - WIDTH) / 2.0).max(0.0);
        ui.horizontal(|ui| {
            ui.add_space(margin);
            ui.vertical(|ui| {
                ui.set_width(WIDTH.min(ui.available_width()));
                ui.add_space(tokens::GAP_WIDE);
                match self.step {
                    Step::Coin => self.coin_step(ui, cx),
                    Step::NewCoin => self.new_coin_step(ui, cx),
                    Step::Mining => self.mining_step(ui, cx),
                    Step::Words => self.words_step(ui, cx),
                }
            });
        });
    }
}

/// Basic or expert words, chosen once here and changed any time in Settings. The two stand side by
/// side on the same three measures behind Details.
fn mode_choice(ui: &mut Ui, cx: &mut Cx) {
    let t = cx.t;
    section(ui, cx.say("setup.mode"));
    ui.add_space(4.0);
    let items = [
        (Mode::Basic, cx.say("settings.mode_basic").to_string()),
        (Mode::Expert, cx.say("settings.mode_expert").to_string()),
    ];
    if let Some(mode) = segments(ui, t, &items, Some(cx.settings.mode)) {
        cx.settings.mode = mode;
    }
    explain::short(ui, t, cx.say("setup.mode_about"));
    let labels = (cx.say("explain.details"), cx.say("explain.hide"));
    explain::details(ui, t, "setup-mode", labels, |ui| modes_compared(ui, cx));
}

/// The two modes on the same measures, which Settings shows too.
pub fn modes_compared(ui: &mut Ui, cx: &Cx) {
    let say = |key| cx.say(key);
    let criteria = [say("mode.words"), say("mode.controls"), say("mode.places")];
    let basic = [say("mode.words_basic"), say("mode.controls_basic"), say("mode.places_same")];
    let expert = [say("mode.words_expert"), say("mode.controls_expert"), say("mode.places_same")];
    let choices = [
        Choice { name: say("settings.mode_basic"), values: &basic },
        Choice { name: say("settings.mode_expert"), values: &expert },
    ];
    explain::compare(ui, cx.t, "modes", &criteria, &choices);
}
