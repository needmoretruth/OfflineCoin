//! Coin rules, forks, locks, board wallets and node kinds, in Korean at the window's first size,
//! in both themes, when `OLC_RULE_SHOTS` names a folder. The coin, its nodes and its blocks are
//! real. Boards are staged, since no test may touch a port.
use super::shots::{shot, tap, with_boards};
use super::{
    app, click, harness, node_program, port_of_scene, quiet_settings, scratch, type_into, wait_until, Window,
    SETTLE_STEPS, SIZES,
};
use crate::core::host_locks::hash_lock_for;
use crate::core::{coin_folder, save_coin, Command, LockOrder, Snapshot};
use crate::i18n::{self, Lang, Mode, Voice};
use crate::local::{Holding, NodeSpec, Preset};
use crate::settings::{Device, Paths, Settings, Theme};
use egui::accesskit::Role;
use egui_kittest::kittest::{By, Queryable};
use olc_core::coin::COIN;
use olc_core::CoinConfig;
use olc_node::Keeps;
use std::path::Path;

fn shelf() -> CoinConfig {
    let mut coin = CoinConfig::olc();
    coin.name = "Shelf".into();
    coin.ticker = "SHF".into();
    coin.message = "Shelf genesis".into();
    coin.initial_difficulty = 2_000;
    coin.rules.min_difficulty = 1;
    coin.rules.floor_from_height = 0;
    coin.rules.retarget_interval = 0;
    coin.rules.block_time_ms = 500;
    coin.rules.coinbase_maturity = 1;
    coin
}

fn kinds() -> Vec<NodeSpec> {
    let named = |name: &str, spec| NodeSpec { name: name.into(), ..spec };
    vec![
        named("작업실 풀 노드", Preset::Esp32S3.spec(0)),
        named("창고 프루닝", NodeSpec { holding: Holding::Recent, ..Preset::Custom.spec(1) }),
        named("주방 라이트", NodeSpec { holding: Holding::Headers, ..Preset::Custom.spec(2) }),
    ]
}

fn with_wallets(live: &Snapshot) -> Option<Snapshot> {
    let mut stage = with_boards(live)?;
    stage.boards.push(super::staged_board(true, true));
    let names = ["내 보드 2", "복도 보드", "창고 보드", "다락 보드"];
    let keeps = [Keeps::Everything, Keeps::Recent, Keeps::Headers, Keeps::Unsaid];
    for (index, board) in stage.boards.iter_mut().enumerate() {
        let number = u8::try_from(index).unwrap_or(0) + 1;
        board.seen.id = [number; 6];
        board.keeps = keeps[index];
        board.seen.beacon.name = names[index].into();
        board.seen.beacon.balance = u64::from(number) * 1_250 * COIN + 5 * COIN / 10;
        board.seen.beacon.address = [number; 20];
        board.seen.beacon.build = "i8".into();
    }
    Some(stage)
}

fn picture(window: &mut Window, folder: &Path, name: &str) {
    let mode = window.state().settings().mode;
    for theme in [Theme::Dark, Theme::Light] {
        window.state_mut().settings_mut().theme = theme;
        window.run_steps(2);
        shot(window, folder, &format!("{name}-{mode:?}-{theme:?}").to_lowercase());
    }
    window.state_mut().settings_mut().theme = Theme::Dark;
}

fn word(window: &Window, key: &str) -> String {
    let settings = window.state().settings();
    i18n::text(Voice { lang: settings.language, mode: settings.mode }, key).to_string()
}

fn scroll_to(window: &mut Window, key: &str) {
    let name = word(window, key);
    if let Some(node) = window.query_all_by_label(&name).next() {
        node.scroll_to_me();
    }
    window.run_steps(SETTLE_STEPS);
}

/// Types into the field a screen reader knows by this name.
fn fill(window: &mut Window, key: &str, text: &str) {
    let name = word(window, key);
    window.get(By::new().role(Role::TextInput).label(&name)).focus();
    window.run_steps(2);
    window.get(By::new().role(Role::TextInput).label(&name)).type_text(text);
    window.run_steps(SETTLE_STEPS);
}

struct Scene<'a> {
    window: &'a mut Window,
    folder: &'a Path,
}

impl Scene<'_> {
    /// Opens `page` afresh in `mode`, walks `go`, and takes the picture.
    fn draw(&mut self, name: &str, page: &str, mode: Mode, go: impl FnOnce(&mut Window)) {
        let window = &mut *self.window;
        window.state_mut().settings_mut().mode = mode;
        window.state_mut().open(page);
        window.run_steps(SETTLE_STEPS);
        go(window);
        picture(window, self.folder, name);
        window.key_press(egui::Key::Escape);
        window.run_steps(2);
        window.key_press(egui::Key::Escape);
        window.run_steps(SETTLE_STEPS);
    }

    fn both(&mut self, name: &str, page: &str, go: fn(&mut Window)) {
        self.draw(name, page, Mode::Basic, go);
        self.draw(name, page, Mode::Expert, go);
    }

    fn stage(&mut self, stage: Option<Snapshot>) {
        self.window.state_mut().set_stage(stage);
        self.window.run_steps(SETTLE_STEPS);
    }

    fn coins(&mut self) {
        self.both("coins-kinds", "page.coins", |_| {});
        self.draw("coins-table", "page.coins", Mode::Expert, |window| scroll_to(window, "found.keeps"));
        self.both("coins-new", "page.coins", |window| tap(window, "coins.new"));
        self.draw("coins-new-end", "page.coins", Mode::Expert, |window| {
            tap(window, "coins.new");
            scroll_to(window, "pro.floor_from_height");
        });
        self.draw("coins-new-bad", "page.coins", Mode::Expert, |window| {
            tap(window, "coins.new");
            type_into(window, 0, "Quick Coin");
            fill(window, "pro.block_ms", "soon");
        });
        self.both("setup-free", "page.coins", |window| {
            tap(window, "coins.new");
            fill(window, "coin.reward", "00");
        });
        self.draw("coins-fork", "page.coins", Mode::Expert, |window| tap(window, "pro.fork"));
        self.draw("coins-fork-slip", "page.coins", Mode::Expert, |window| {
            tap(window, "pro.fork");
            fill(window, "coin.reward", "25");
            tap(window, "pro.fork_go");
        });
    }

    fn locks(&mut self) {
        let page = "page.locks";
        self.draw("locks", page, Mode::Expert, |_| {});
        self.draw("locks-hash", page, Mode::Expert, |window| {
            tap(window, "lock.hash");
            type_into(window, 0, "a long way home");
        });
        self.draw("locks-two", page, Mode::Expert, |window| tap(window, "lock.two_of_two"));
        self.draw("locks-time", page, Mode::Expert, |window| tap(window, "lock.time"));
        self.window.state().send(Command::Lock(LockOrder::Pay {
            words: hash_lock_for("a long way home"),
            amount: "5".into(),
        }));
        wait_until(self.window, |app| !app.snapshot().node.locks.is_empty());
        self.draw("locks-paid", page, Mode::Expert, |window| scroll_to(window, "lock.coins"));
        self.draw("locks-spend", page, Mode::Expert, |window| tap(window, "lock.spend"));
    }

    fn boards(&mut self) {
        let live = self.window.state().snapshot();
        self.stage(with_wallets(&live));
        let page = "page.boards";
        self.both("boards-wallets", page, |_| {});
        self.both("board-detail", page, |window| click(window, "내 보드 2"));
        self.both("board-receive", page, |window| {
            click(window, "내 보드 2");
            scroll_to(window, "boards.show_code");
            tap(window, "boards.show_code");
        });
        self.both("board-offline", page, |window| click(window, "창고 보드"));
        self.both("board-returning", page, |window| click(window, "다락 보드"));
        self.both("send-wallets", "page.home", |window| tap(window, "home.send"));
        self.both("send-from-board", "page.home", |window| {
            tap(window, "home.send");
            click(window, "내 보드 2");
            let address = window.state().snapshot().node.address.clone();
            type_into(window, 0, &address);
            type_into(window, 1, "12.5");
            tap(window, "send.next");
        });
        self.stage(None);
    }

    fn cards(&mut self) {
        self.both("node-cards", "page.boards", |window| tap(window, "boards.tab_nodes"));
        self.both("node-card-light", "page.boards", |window| {
            tap(window, "boards.tab_nodes");
            window.run_steps(SETTLE_STEPS);
            if let Some(card) = window.query_all_by_label("주방 라이트").next() {
                card.scroll_to_me();
            }
            window.run_steps(SETTLE_STEPS);
        });
        self.both("bluetooth", "page.settings", |window| scroll_to(window, "boards.bluetooth"));
    }

    /// A node that stopped by itself, with what the system said about it.
    fn stopped(&mut self) {
        let mut stage = Snapshot::clone(&self.window.state().snapshot());
        if let Some(node) = stage.local.first_mut() {
            node.pid = None;
            node.status = None;
            node.measured = None;
            node.ended = Some("exit status: 1: could not open the chain folder".into());
        }
        self.stage(Some(stage));
        self.both("node-stopped", "page.boards", |window| tap(window, "boards.tab_nodes"));
        self.stage(None);
    }

    fn fork(&mut self) {
        let height = self.window.state().snapshot().node.height;
        let changes = vec![("rules.reward".to_string(), "25".to_string())];
        self.window.state().send(Command::Fork { at: Some(height + 4), changes });
        let forked = |snap: &Snapshot| snap.node.coin.as_ref().is_some_and(|coin| !coin.forks.is_empty());
        wait_until(self.window, |app| forked(&app.snapshot()));
        self.both("coins-forked", "page.coins", |_| {});
        self.draw("coins-fork-back", "page.coins", Mode::Expert, |window| tap(window, "coins.fork_back"));
        self.window.state().send(Command::ForkBack);
        wait_until(self.window, |app| !forked(&app.snapshot()));
    }
}

fn setup_form(folder: &Path) {
    let settings = Settings { setup_done: false, port: port_of_scene(80), ..quiet_settings() };
    let mut first = harness(app(scratch("rule-shots-first"), settings, None), SIZES[0]);
    click(&mut first, "ESP32 boards");
    click(&mut first, "New coin");
    first.state_mut().settings_mut().language = Lang::Ko;
    first.run_steps(SETTLE_STEPS);
    picture(&mut first, folder, "setup-new-coin");
    first.state_mut().settings_mut().mode = Mode::Expert;
    first.run_steps(SETTLE_STEPS);
    picture(&mut first, folder, "setup-new-coin");
}

#[test]
fn rule_screens() {
    let Some(folder) = std::env::var_os("OLC_RULE_SHOTS").map(std::path::PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&folder).unwrap();
    setup_form(&folder);
    let root = scratch("rule-shots");
    let paths = Paths { root: root.clone() };
    let coin = shelf();
    save_coin(&paths, &coin).unwrap();
    let mut settings = quiet_settings();
    settings.language = Lang::Ko;
    settings.coin = Some(coin_folder(&coin));
    settings.device = Device::ThisComputer;
    settings.mine_on_start = true;
    settings.hash_cap = 10_000;
    settings.port = port_of_scene(82);
    settings.nodes = kinds();
    let mut live = harness(app(root, settings, Some(node_program())), SIZES[0]);
    wait_until(&mut live, |app| {
        let snap = app.snapshot();
        let running = snap.local.iter().filter(|view| view.status.is_some()).count() == 3;
        running && snap.node.spendable >= 20 * COIN
    });
    live.run_steps(SETTLE_STEPS);
    let mut scene = Scene { window: &mut live, folder: &folder };
    scene.coins();
    scene.locks();
    scene.boards();
    scene.cards();
    scene.stopped();
    scene.fork();
}
