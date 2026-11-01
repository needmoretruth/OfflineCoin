//! The window: it holds the pages, reads a snapshot of the core, and sends commands back.
use crate::core::{Command, Core, CoreLaunch, Snapshot};
use crate::fleet::{FleetPort, RealFleet};
use crate::i18n::Lang;
use crate::instance::{claim, Instance};
use crate::pages::setup::Setup;
use crate::pages::{all, Cx, Page};
use crate::settings::{standard_folder, Paths, Settings};
use crate::ui::logo::{render, Logo};
use crate::ui::theme::{apply, desktop_reduces_motion, tokens_for};
use crate::ui::tokens::{self, Tokens, DARK};
use crate::ui::{fonts, shell};
use eframe::egui::{self, CentralPanel, Frame, IconData, Margin, ScrollArea, Ui, ViewportBuilder};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

const APP_ID: &str = "olc-core";
const WINDOW: [f32; 2] = [1280.0, 800.0];
const WINDOW_MIN: [f32; 2] = [960.0, 620.0];
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
    pages: Vec<Box<dyn Page>>,
    current: usize,
    setup: Setup,
    logo: Logo,
    tokens: Tokens,
    last_theme: Option<(bool, crate::settings::Theme)>,
    notice_until: Option<(u64, f64)>,
    instance: Option<Instance>,
    fonts_set: bool,
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

    fn language(&mut self) {
        self.settings.language = self.settings.language.other();
        self.core.send(Command::Save(Box::new(self.settings.clone())));
    }

    fn show_notice(&mut self, ui: &mut Ui, t: &Tokens, lang: Lang, snap: &Snapshot) {
        let Some(notice) = &snap.notice else {
            return;
        };
        let now = ui.input(|input| input.time);
        let until = match self.notice_until {
            Some((number, until)) if number == notice.number => until,
            _ => now + tokens::NOTICE_SECONDS,
        };
        self.notice_until = Some((notice.number, until));
        if now < until {
            shell::notice_bar(ui, t, lang, notice);
            ui.ctx().request_repaint_after_secs((until - now) as f32);
        }
    }

    fn pages_view(&mut self, ui: &mut Ui, snap: &Snapshot) {
        let t = self.tokens;
        let lang = self.settings.language;
        let frame = Frame::new().inner_margin(Margin::symmetric(tokens::PAGE_MARGIN, 18));
        CentralPanel::default().frame(frame).show(ui, |ui| {
            ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                ui.set_max_width(tokens::PAGE_WIDTH.min(ui.available_width()));
                let mut cx = Cx {
                    lang,
                    snap,
                    settings: &mut self.settings,
                    t: &t,
                    core: &self.core,
                };
                if cx.settings.setup_done {
                    self.pages[self.current].show(ui, &mut cx);
                } else {
                    self.setup.show(ui, &mut cx);
                }
            });
        });
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
        let t = self.tokens;
        let lang = self.settings.language;
        let snap: Arc<Snapshot> = self.core.snapshot();
        shell::paint_back(ui, &t);
        let look = shell::HeaderLook {
            still: self.settings.reduce_motion || desktop_reduces_motion(),
            strip: self.settings.setup_done,
        };
        let head = shell::header(ui, &t, lang, &snap, &mut self.logo, &look);
        if head.language_clicked {
            self.language();
        }
        if self.settings.setup_done {
            if let Some(page) = shell::side_bar(ui, &t, lang, &self.pages, self.current) {
                self.current = page;
            }
        }
        self.show_notice(ui, &t, lang, &snap);
        self.pages_view(ui, &snap);
        let focused = ctx.input(|input| input.viewport().focused.unwrap_or(true));
        ctx.request_repaint_after(if focused { REPAINT_FOCUSED } else { REPAINT_AWAY });
    }

    /// Goes to the page with this name, for tests of the screens.
    pub fn open(&mut self, title: &str) {
        if let Some(index) = self.pages.iter().position(|page| page.title() == title) {
            self.current = index;
        }
    }

    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.core.snapshot()
    }

    pub fn settings_mut(&mut self) -> &mut Settings {
        &mut self.settings
    }

    pub fn page_titles(&self) -> Vec<&'static str> {
        self.pages.iter().map(|page| page.title()).collect()
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }
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
        .then(|| RealFleet::new(launch.ports.clone()).ok())
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
    eframe::run_native(
        "OLC Core",
        options,
        Box::new(move |_| Ok(Box::new(App::new(core, settings, Some(instance))))),
    )
    .map_err(|error| error.to_string())
}
