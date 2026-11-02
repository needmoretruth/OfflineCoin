//! The window: it holds the pages, reads a snapshot of the core, and sends commands back.
use crate::core::{Core, CoreLaunch, Snapshot};
#[cfg(test)]
use crate::core::Command;
use crate::fleet::{FleetPort, RealFleet};
use crate::i18n::{self, Mode, Voice};
use crate::instance::{claim, Instance};
use crate::pages::setup::Setup;
use crate::pages::{all, Cx};
use crate::settings::{standard_folder, Paths, Settings};
use crate::ui::floating::{self, show_slip, Slip, SlipAnswer};
use crate::ui::logo::{render, Logo};
use crate::ui::theme::{apply, desktop_reduces_motion, tokens_for};
use crate::ui::tokens::{self, Tokens, DARK};
use crate::ui::{fonts, shell};
use eframe::egui::{self, CentralPanel, Frame, IconData, Margin, Panel, ScrollArea, Ui, ViewportBuilder};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

const APP_ID: &str = "olc-core";
const WINDOW: [f32; 2] = [1280.0, 800.0];
const WINDOW_MIN: [f32; 2] = [960.0, 620.0];
/// The share of the screen a new window may take, leaving room for a taskbar or the Dock.
const SCREEN_SHARE: f32 = 0.9;
const ICON_PIXELS: usize = 128;
const REPAINT_FOCUSED: Duration = Duration::from_secs(1);
const REPAINT_AWAY: Duration = Duration::from_secs(3);

pub struct Launch {
    /// False leaves every USB port alone.
    pub usb: bool,
    /// The only USB ports to use. Empty finds every board by itself.
    pub ports: Vec<String>,
    pub data: Option<PathBuf>,
}

impl Default for Launch {
    fn default() -> Self {
        Self {
            usb: true,
            ports: Vec::new(),
            data: None,
        }
    }
}

pub struct App {
    core: Core,
    settings: Settings,
    pages: Vec<Box<dyn crate::pages::Page>>,
    current: usize,
    setup: Setup,
    logo: Logo,
    tokens: Tokens,
    last_theme: Option<(bool, crate::settings::Theme)>,
    notice_until: Option<(u64, f64)>,
    instance: Option<Instance>,
    fonts_set: bool,
    /// Whether the window has been checked against the screen it opened on.
    fitted: bool,
    /// What is waiting to be read and signed.
    slip: Option<Slip>,
    /// What the pictures and tests show in place of the core's own view of boards.
    #[cfg(test)]
    stage: Option<Arc<Snapshot>>,
    /// Where the last file was chosen.
    folder: Option<PathBuf>,
    text_percent: u16,
}

impl App {
    pub fn new(core: Core, settings: Settings, instance: Option<Instance>) -> Self {
        Self {
            core,
            settings,
            pages: all(),
            current: 0,
            setup: Setup::default(),
            logo: Logo::default(),
            tokens: DARK,
            last_theme: None,
            notice_until: None,
            instance,
            fonts_set: false,
            fitted: false,
            slip: None,
            #[cfg(test)]
            stage: None,
            folder: None,
            text_percent: 100,
        }
    }

    fn theme(&mut self, ctx: &egui::Context) {
        self.tokens = tokens_for(ctx, self.settings.theme);
        let key = (self.tokens.dark, self.settings.theme);
        if self.last_theme != Some(key) {
            self.last_theme = Some(key);
            apply(ctx, &self.tokens);
        }
    }

    fn raised(&self, ctx: &egui::Context) {
        let asked = self.instance.as_ref().is_some_and(|i| i.raised.try_recv().is_ok());
        if asked {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
    }

    fn text_size(&mut self, ctx: &egui::Context) {
        let percent = self.settings.text_percent;
        if percent != self.text_percent {
            self.text_percent = percent;
            ctx.set_zoom_factor(f32::from(percent) / 100.0);
        }
    }

    fn show_notice(&mut self, ui: &Ui, t: &Tokens, voice: Voice, snap: &Snapshot) {
        let Some(notice) = &snap.notice else {
            return;
        };
        let now = ui.input(|input| input.time);
        let seconds = if notice.good { tokens::NOTICE_SECONDS } else { tokens::FAILURE_SECONDS };
        let until = match self.notice_until {
            Some((number, until)) if number == notice.number => until,
            _ => now + seconds,
        };
        self.notice_until = Some((notice.number, until));
        if now < until {
            let colour = if notice.good { t.good } else { t.bad };
            let line = i18n::fill(i18n::text(voice, notice.key), &[notice.detail.as_str()]);
            floating::notice(ui, t, colour, &line);
            ui.ctx().request_repaint_after_secs((until - now) as f32);
        }
    }

    fn pages_view(&mut self, ui: &mut Ui, snap: &Snapshot) {
        let t = self.tokens;
        let lang = self.settings.language;
        let done = self.settings.setup_done;
        let wide = ui.available_width() >= tokens::NARROW_WINDOW;
        let margin = if wide { tokens::PAGE_MARGIN } else { tokens::PAGE_MARGIN_NARROW };
        // The margin sits inside the scroll area, so the scroll bar stays at the window's edge, clear
        // of the cards.
        let page = Frame::new().inner_margin(Margin { left: margin, right: margin, top: 22, bottom: 0 });
        let mut go = None;
        CentralPanel::default().frame(Frame::new()).show(ui, |ui| {
            ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                page.show(ui, |ui| {
                    ui.set_max_width(tokens::PAGE_WIDTH.min(ui.available_width()));
                    let mut cx = Cx {
                        lang,
                        snap,
                        settings: &mut self.settings,
                        t: &t,
                        core: &self.core,
                        slip: &mut self.slip,
                        go: &mut go,
                        folder: &mut self.folder,
                    };
                    if done {
                        self.pages[self.current].show(ui, &mut cx);
                    } else {
                        self.setup.show(ui, &mut cx);
                    }
                    ui.add_space(tokens::GAP_PAGE);
                });
            });
        });
        if let Some(title) = go {
            self.open(title);
        }
    }

    fn show_slip(&mut self, ui: &Ui, t: &Tokens) {
        let Some(slip) = &self.slip else {
            return;
        };
        match show_slip(ui, t, slip) {
            SlipAnswer::Waiting => {}
            SlipAnswer::Cancelled => self.slip = None,
            SlipAnswer::Signed => {
                for order in self.slip.take().into_iter().flat_map(|slip| slip.order) {
                    self.core.send(order);
                }
            }
        }
    }

    /// The first run has no places yet, only the name of what is opening.
    fn first_run_brand(&mut self, ui: &mut Ui, t: &Tokens, snap: &Snapshot) {
        let still = self.settings.reduce_motion || desktop_reduces_motion();
        let frame = Frame::new().inner_margin(Margin::symmetric(tokens::PAGE_MARGIN, 20));
        Panel::top("brand").frame(frame).show_separator_line(false).show(ui, |ui| {
            shell::brand(ui, t, snap, &mut self.logo, still);
        });
    }

    fn rail(&mut self, ui: &mut Ui, t: &Tokens, voice: Voice, snap: &Snapshot) {
        let expert = voice.mode == Mode::Expert;
        if self.pages[self.current].expert_only() && !expert {
            self.current = 0;
        }
        let still = self.settings.reduce_motion || desktop_reduces_motion();
        let shown: Vec<(usize, &dyn crate::pages::Page)> = self
            .pages
            .iter()
            .enumerate()
            .filter(|(_, page)| expert || !page.expert_only())
            .map(|(index, page)| (index, page.as_ref()))
            .collect();
        let chosen = shell::rail(ui, t, voice, snap, &shown, self.current, (&mut self.logo, still));
        if let Some(page) = chosen {
            self.current = page;
        }
    }

    /// Everything inside the window, also what the screen tests draw.
    pub fn draw(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        // New fonts apply from the next frame, and the first frame has none to draw with.
        if !self.fonts_set {
            self.fonts_set = true;
            ctx.set_fonts(fonts::definitions());
            ctx.request_repaint();
            return;
        }
        self.theme(&ctx);
        self.raised(&ctx);
        self.text_size(&ctx);
        let t = self.tokens;
        let voice = Voice { lang: self.settings.language, mode: self.settings.mode };
        let snap: Arc<Snapshot> = self.view();
        shell::paint_back(ui, &t);
        if self.settings.setup_done {
            self.rail(ui, &t, voice, &snap);
        } else {
            self.first_run_brand(ui, &t, &snap);
        }
        self.pages_view(ui, &snap);
        self.show_slip(ui, &t);
        self.show_notice(ui, &t, voice, &snap);
        let focused = ctx.input(|input| input.viewport().focused.unwrap_or(true));
        ctx.request_repaint_after(if focused { REPAINT_FOCUSED } else { REPAINT_AWAY });
    }

    /// Goes to the page with this name, for tests of the screens.
    pub fn open(&mut self, title: &str) {
        if let Some(index) = self.pages.iter().position(|page| page.title() == title) {
            self.current = index;
        }
    }

    #[cfg(test)]
    pub fn send(&self, command: Command) {
        self.core.send(command);
    }

    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.core.snapshot()
    }

    #[cfg(not(test))]
    fn view(&self) -> Arc<Snapshot> {
        self.core.snapshot()
    }

    #[cfg(test)]
    fn view(&self) -> Arc<Snapshot> {
        self.stage.clone().unwrap_or_else(|| self.core.snapshot())
    }

    /// Shows this in place of the core's view, for boards no test can have.
    #[cfg(test)]
    pub fn set_stage(&mut self, stage: Option<Snapshot>) {
        self.stage = stage.map(Arc::new);
    }

    /// Where the file sheets start, for tests that may not touch a real home folder.
    #[cfg(test)]
    pub fn set_folder(&mut self, folder: PathBuf) {
        self.folder = Some(folder);
    }

    #[cfg(test)]
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// The settings, for tests of the screens.
    pub fn settings_mut(&mut self) -> &mut Settings {
        &mut self.settings
    }

    pub fn page_titles(&self) -> Vec<&'static str> {
        self.pages.iter().map(|page| page.title()).collect()
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        if !self.fitted {
            self.fitted = fit_to_screen(ui.ctx());
        }
        self.draw(ui);
    }
}

/// A window taller or wider than the screen loses its edge under the taskbar or the Dock, so it
/// shrinks to fit and moves to the middle. Returns true once the screen's size is known.
fn fit_to_screen(ctx: &egui::Context) -> bool {
    let Some(screen) = ctx.input(|input| input.viewport().monitor_size) else {
        return false;
    };
    let full = egui::vec2(WINDOW[0], WINDOW[1]);
    let least = egui::vec2(WINDOW_MIN[0], WINDOW_MIN[1]);
    let size = full.min(screen * SCREEN_SHARE).max(least);
    if size != full {
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        let corner = ((screen - size) / 2.0).max(egui::Vec2::ZERO);
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(corner.to_pos2()));
    }
    true
}

fn data_folder(launch: &Launch) -> Result<PathBuf, String> {
    if let Some(data) = &launch.data {
        return Ok(data.clone());
    }
    standard_folder()
}

fn window_icon() -> IconData {
    let image = render(0.4, ICON_PIXELS);
    let pixels = image.pixels.iter();
    let rgba = pixels.flat_map(|pixel| pixel.to_srgba_unmultiplied()).collect();
    IconData {
        rgba,
        width: ICON_PIXELS as u32,
        height: ICON_PIXELS as u32,
    }
}

fn fleet_of(launch: &Launch) -> Option<Arc<dyn FleetPort>> {
    launch
        .usb
        .then(|| RealFleet::new(launch.ports.clone(), None).ok())
        .flatten()
        .map(|fleet| Arc::new(fleet) as Arc<dyn FleetPort>)
}

pub fn run(launch: Launch) -> Result<(), String> {
    let root = data_folder(&launch)?;
    let Some(instance) = claim(&root)? else {
        return Ok(());
    };
    let paths = Paths { root: root.clone() };
    let settings = Settings::load(&root);
    let usb = launch.usb;
    let fleet = fleet_of(&launch);
    let core = Core::start(
        CoreLaunch {
            paths,
            fleet,
            usb,
            node_program: None,
        },
        settings.clone(),
    )?;
    let viewport = ViewportBuilder::default()
        .with_app_id(APP_ID)
        .with_title("OLC Core")
        .with_inner_size(WINDOW)
        .with_min_inner_size(WINDOW_MIN)
        .with_icon(window_icon());
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    let closing = core.clone();
    let ran = eframe::run_native(
        "OLC Core",
        options,
        Box::new(move |_| Ok(Box::new(App::new(core, settings, Some(instance))))),
    )
    .map_err(|error| error.to_string());
    closing.finish();
    ran
}
