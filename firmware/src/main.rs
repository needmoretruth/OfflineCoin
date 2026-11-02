//! An OfflineCoin node for the ESP32-S3. It mines, checks every block and payment, keeps its chain
//! in flash, and talks over ESP-NOW, Wi-Fi and USB. An OLED, if there is one, shows its state.
#![no_std]
#![no_main]
extern crate alloc;

mod ble;
mod clock;
mod commands;
mod control;
mod flash;
mod hangul;
mod links;
mod node;
mod pages;
mod radio;
mod report;
mod screen;
mod serve;
mod store;
mod thermo;
mod uart;
mod usb;
mod views;
mod wifi;
mod words;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use control::Control;
use embassy_executor::Spawner;
use embassy_futures::yield_now;
use embassy_time::Timer;
use esp_backtrace as _;
use esp_hal::efuse::{self, InterfaceMacAddress};
use esp_hal::gpio::{Input, InputConfig, Pull};
use esp_hal::i2c::master::{Config as I2cConfig, I2c};
use esp_hal::peripherals::LPWR;
use esp_hal::rng::Rng;
use esp_hal::rtc_cntl::{Rtc, RwdtStage, RwdtStageAction};
use esp_hal::time::{Duration, Instant, Rate};
use esp_hal::timer::timg::TimerGroup;
use esp_hal::usb::usb_serial_jtag::UsbSerialJtag;
use esp_println::println;
use esp_storage::FlashStorage;
use node::Node;
use olc_control::bootlog::Stage;
use olc_core::{Chain, CoinConfig, Plugins};
use olc_crypto::Signer;
use olc_node::{Engine, Rank, Runner, MOST_HEADERS_ON_A_BOARD};
use olc_pow::ProofOfWork;
use olc_wallet::{format_address, parse_address, Wallet};
use radio::RadioLink;
use screen::Screen;
use store::Store;
use usb::UsbLink;
use views::{Coins, Language, View};

esp_bootloader_esp_idf::esp_app_desc!();

/// Blocks loaded between two drawings of the start screen.
/// Rows of the logo drawn for each block read back, so the chain's load still leads.
const LOAD_COIN_ROWS: usize = 3;
/// How often the panel moves while old blocks load. Drawing takes time from the blocks.
const LOAD_FRAME_MS: u64 = 500;
/// The supply settles after the panel's first light before Wi-Fi starts, which draws the most.
const SETTLE_MS: u64 = 300;
/// And longer after the last start ran it dry.
const SETTLE_AFTER_BROWN_OUT_MS: u64 = 2_000;
const BROWN_OUT: u8 = 0x0F;
/// Far longer than a healthy start takes to reach the radio.
const START_WATCHDOG: Duration = Duration::from_secs(20);

fn own_network_name() -> String {
    let mac = efuse::interface_mac_address(InterfaceMacAddress::Station);
    let mac = mac.as_bytes();
    format!("OLC-{:02X}{:02X}", mac[4], mac[5])
}

/// The cap the owner chose, if any. A saved 0 from before `cap` existed only meant unset.
fn chosen_limit(settings: &store::Settings) -> Option<u32> {
    let old = settings.limit.parse().ok().filter(|limit| *limit != 0);
    settings.cap.parse().ok().or(old)
}

/// Kept free while the chain loads, then given back. The allocator walks its list of free
/// stretches from the front, so one long stretch there keeps every later allocation quick.
const ROOM_AT_THE_FRONT: usize = 1024 * 1024;

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    // What the steps before this program left behind. `init` turns the watchdog off.
    let boot_watchdog = LPWR::regs().wdtconfig0().read().wdt_en().bit_is_set();
    let clock_mhz = clock::running_mhz();
    let peripherals =
        esp_hal::init(esp_hal::Config::default().with_cpu_clock(clock::cpu_clock(clock_mhz)));

    // A board with no cable has nobody to press its reset button. It restarts itself if it stops
    // before the radio is up, and the word below shows how far the stopped start got.
    let mut rtc = Rtc::new(peripherals.RTC_TIMER);
    rtc.rwdt.set_timeout(RwdtStage::Stage0, START_WATCHDOG);
    rtc.rwdt
        .set_stage_action(RwdtStage::Stage0, RwdtStageAction::ResetSystem);
    rtc.rwdt.enable();
    flash::give(FlashStorage::new(peripherals.FLASH));
    let mut store = Store::default();
    let reset = esp_hal::system::reset_reason().map_or(0, |reason| reason as u8);
    store.begin_boot(reset);

    // PSRAM comes first, so the chain lands there and internal memory stays free for the radio.
    esp_alloc::psram_allocator!(peripherals.PSRAM, esp_hal::psram);
    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 73744);
    esp_alloc::heap_allocator!(size: 64 * 1024);
    let room = alloc::vec::Vec::<u8>::with_capacity(ROOM_AT_THE_FRONT);
    core::hint::black_box(&room);
    store.reach(Stage::Psram);

    let timers = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timers.timer0, peripherals.FROM_CPU_INTR0);

    let mut usb_device = UsbSerialJtag::new(peripherals.USB_DEVICE);
    usb_device.set_interrupt_handler(usb::on_usb_packet);
    let (rx, _tx) = usb_device.split();
    let bus = I2c::new(
        peripherals.I2C0,
        I2cConfig::default().with_frequency(Rate::from_khz(400)),
    )
    .expect("the I2C settings are valid")
    .with_sda(peripherals.GPIO8)
    .with_scl(peripherals.GPIO9);

    // A long chain takes a while to load, and a dark panel looks like a dead board.
    thermo::start();
    let settings = store.settings();
    if clock::saved_mhz(&settings) != clock_mhz {
        clock::restart_at(clock::saved_mhz(&settings));
    }
    let bright = settings.bright.parse().unwrap_or(screen::DEFAULT_BRIGHT);
    let mut screen = Screen::new(bus);
    screen.set_bright(bright);
    let mut coins = Coins::new();
    coins.first_frame();
    views::splash(&mut screen, &coins, 0);
    let panel_ms = node::device_ms();
    store.reach(Stage::Panel);

    let coin = store.coin().unwrap_or_else(CoinConfig::olc);
    let seed = u64::from(Rng::new().random()) << 32 | u64::from(Rng::new().random());
    let name = own_network_name();
    let power_dbm = settings.power.parse().unwrap_or(node::DEFAULT_POWER_DBM);
    let settle = match reset {
        BROWN_OUT => SETTLE_AFTER_BROWN_OUT_MS,
        _ => SETTLE_MS,
    };
    Timer::after_millis(settle).await;
    store.reach(Stage::Panel);
    let (espnow, wifi_link, network) =
        wifi::start(spawner, peripherals.WIFI, &settings, &name, power_dbm, seed);
    let wired = uart::start(peripherals.UART1, peripherals.UART2, peripherals.GPIO17, peripherals.GPIO18);

    store.reach(Stage::Radio);
    rtc.rwdt.disable();

    let (secrets, receiving) = store.keys(|| {
        // The radio is on, so this is true hardware randomness.
        let mut secret = [0u8; 32];
        Rng::new().read(&mut secret);
        secret
    });
    let wallet = Wallet::from_secrets(&secrets, receiving).expect("saved keys are valid");
    let secret = secrets[receiving.min(secrets.len() - 1)];
    let signer = Signer::new(&secret).expect("a saved key is valid");
    let plugins = Plugins {
        consensus: Box::new(ProofOfWork),
        languages: olc_lang::all(),
    };
    let old_blocks = store.old_blocks(&coin.genesis().id());
    let keep = store.keep(settings.recent.parse().ok());
    let read_ms = node::device_ms();
    links::take_blocks_of(coin.rules.max_block_bytes);
    let chain = Chain::open(coin, plugins, keep);
    let mut engine = Engine::on_chain(chain, wallet);
    engine.set_miner(parse_address(&settings.miner));
    engine.set_note(&commands::note_bytes(&settings.note));
    engine.set_merging(settings.merge != "off");
    let light = settings.light == "on";
    if light {
        engine.become_light(MOST_HEADERS_ON_A_BOARD, &[]);
    }
    store.reach(Stage::ChainRead);
    let placed_ms = node::device_ms();
    let total = old_blocks.len();
    let mut frame_ms = 0;
    for (done, block) in old_blocks.into_iter().enumerate() {
        let _ = engine.chain_mut().add_block(block);
        coins.work_on_logo(LOAD_COIN_ROWS);
        let now = node::device_ms();
        if now >= frame_ms {
            frame_ms = now + LOAD_FRAME_MS;
            views::loading(&mut screen, &coins, now, done, total);
            println!("moving old blocks {done} of {total}");
        }
    }
    store.forget_old_blocks();
    if total > 0 {
        engine.chain_mut().save_now();
    }
    let loaded_ms = node::device_ms();
    store.reach(Stage::ChainLoaded);
    println!(
        "started address={} wifi={name}",
        format_address(&engine.address())
    );

    let updated = store.mark_running_build_good();
    let mac = efuse::interface_mac_address(InterfaceMacAddress::Station);
    let mut me = [0u8; 6];
    me.copy_from_slice(mac.as_bytes());
    let identity = store.identity();
    let counter = store.counter();
    let slot = store.running_slot();
    let control = Control::new(me, Rng::new().random(), identity, counter, slot, signer);

    let mut runner = Runner::new(engine);
    runner.add_link(Box::new(RadioLink::new(espnow, runner.mark())));
    let (usb, commands) = UsbLink::new(rx);
    runner.add_link(Box::new(usb));
    runner.add_link(Box::new(wifi_link));
    runner.add_link(Box::new(wired));
    // A board goes by its radio address, which nothing else has.
    let mut id = [0u8; 8];
    id[..6].copy_from_slice(&me);
    runner.set_id(u64::from_le_bytes(id));
    if settings.bluetooth_on() {
        let link = ble::start(spawner, peripherals.BT, runner.id(), me);
        runner.add_link(Box::new(link));
    }
    if let Some(rank) = Rank::parse(&settings.links) {
        runner.set_rank(rank);
    }
    if !settings.dial.is_empty() {
        runner.add_address(node::LINK_WIFI, settings.dial.clone());
    }

    let tip_seen = runner.engine.chain().tip();
    let limit = chosen_limit(&settings);
    let mut node = Node {
        runner,
        control,
        store,
        screen,
        started: node::Started {
            reset,
            panel_ms,
            read_ms,
            placed_ms,
            loaded_ms,
        },
        coins,
        bright,
        boot_watchdog,
        clock_mhz,
        restart_ms: None,
        busy_us: 0,
        busy_percent: 0,
        smooth: settings.smooth.parse().unwrap_or(node::DEFAULT_SMOOTH),
        coin_ms: 0,
        frames: Default::default(),
        power_dbm,
        commands,
        network,
        button: Input::new(
            peripherals.GPIO0,
            InputConfig::default().with_pull(Pull::Up),
        ),
        view: View::from_name(&settings.view).unwrap_or(View::Basic),
        language: Language::from_name(&settings.language).unwrap_or(Language::English),
        mining: !light && settings.mine != "off",
        bluetooth: settings.bluetooth_on(),
        work: None,
        waiting_coin: None,
        limit: limit.unwrap_or(node::RELAXED_LIMIT),
        limit_chosen: limit.is_some(),
        memory_full: false,
        attempts: 0,
        rate: 0,
        last_reorg: (0, 0),
        dropped: Default::default(),
        window_ms: 0,
        tip_attempts: 0,
        tip_seen,
        tip_seen_ms: 0,
        block_time_ms: None,
        mined: 0,
        event: None,
        notes: Default::default(),
        screen_ms: 0,
        status_ms: 0,
        pressed_ms: None,
        splash_until_ms: node::SPLASH_LENGTH_MS,
    };
    node.store.reach(Stage::Running);
    drop(room);
    if updated {
        node.note(0, views::Note::Updated(control::BUILD));
    }
    loop {
        let start = Instant::now();
        let worked = node.turn();
        node.busy_us += start.elapsed().as_micros();
        if worked {
            yield_now().await;
        } else {
            Timer::after_millis(5).await;
        }
    }
}
