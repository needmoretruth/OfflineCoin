//! An OfflineCoin node for the ESP32-S3. It mines, checks every block and payment, keeps its chain
//! in flash, and talks over ESP-NOW, Wi-Fi and USB. An OLED, if there is one, shows its state.
#![no_std]
#![no_main]
extern crate alloc;

mod control;
mod hangul;
mod node;
mod pages;
mod radio;
mod screen;
mod store;
mod thermo;
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
use esp_hal::clock::CpuClock;
use esp_hal::efuse::{self, InterfaceMacAddress};
use esp_hal::gpio::{Input, InputConfig, Pull};
use esp_hal::i2c::master::{Config as I2cConfig, I2c};
use esp_hal::rng::Rng;
use esp_hal::time::Rate;
use esp_hal::timer::timg::TimerGroup;
use esp_hal::usb::usb_serial_jtag::UsbSerialJtag;
use esp_println::println;
use esp_storage::FlashStorage;
use node::Node;
use olc_control::bootlog::Stage;
use olc_core::{CoinConfig, Plugins};
use olc_node::{Engine, Runner};
use olc_pow::ProofOfWork;
use olc_wallet::{format_address, Wallet};
use radio::RadioLink;
use screen::Screen;
use store::Store;
use usb::UsbLink;
use views::{Language, View};

esp_bootloader_esp_idf::esp_app_desc!();

/// Blocks loaded between two drawings of the start screen.
const LOAD_REDRAW_EVERY: usize = 40;

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

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));

    // PSRAM comes first, so the chain lands there and internal memory stays free for the radio.
    esp_alloc::psram_allocator!(peripherals.PSRAM, esp_hal::psram);
    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 73744);
    esp_alloc::heap_allocator!(size: 64 * 1024);

    let timers = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timers.timer0, peripherals.FROM_CPU_INTR0);

    let mut usb_device = UsbSerialJtag::new(peripherals.USB_DEVICE);
    usb_device.set_interrupt_handler(usb::on_usb_packet);
    let (rx, _tx) = usb_device.split();
    let mut store = Store::new(FlashStorage::new(peripherals.FLASH));
    let reset = esp_hal::system::reset_reason().map_or(0, |reason| reason as u8);
    store.begin_boot(reset);
    let bus = I2c::new(
        peripherals.I2C0,
        I2cConfig::default().with_frequency(Rate::from_khz(400)),
    )
    .expect("the I2C settings are valid")
    .with_sda(peripherals.GPIO8)
    .with_scl(peripherals.GPIO9);

    // A long chain takes a while to load, and a dark panel looks like a dead board.
    thermo::start();
    let mut screen = Screen::new(bus);
    views::splash(&mut screen, 0);

    let coin = CoinConfig::olc();
    let settings = store.settings();
    let seed = u64::from(Rng::new().random()) << 32 | u64::from(Rng::new().random());
    let name = own_network_name();
    let (espnow, wifi_link, network) =
        wifi::start(spawner, peripherals.WIFI, &settings, &name, seed);

    store.reach(Stage::Radio);

    let secret = store.secret(&coin.genesis().id(), || {
        // The radio is on, so this is true hardware randomness.
        let mut secret = [0u8; 32];
        Rng::new().read(&mut secret);
        secret
    });
    let wallet = Wallet::from_secret(secret).expect("a random key is valid");
    let plugins = Plugins {
        consensus: Box::new(ProofOfWork),
        languages: olc_lang::all(),
    };
    let mut engine = Engine::new(coin, plugins, wallet);
    let blocks = store.load_blocks();
    let read_ms = node::device_ms();
    store.reach(Stage::ChainRead);
    let total = blocks.len();
    for (done, block) in blocks.into_iter().enumerate() {
        engine.restore_block(block);
        if done % LOAD_REDRAW_EVERY == 0 {
            views::loading(&mut screen, node::device_ms(), done, total);
        }
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
    let control = Control::new(me, Rng::new().random(), identity, counter, slot);

    let mut runner = Runner::new(engine);
    runner.add_link(Box::new(RadioLink::new(espnow, runner.mark())));
    let (usb, commands) = UsbLink::new(rx);
    runner.add_link(Box::new(usb));
    runner.add_link(Box::new(wifi_link));

    let tip_seen = runner.engine.chain().tip();
    let limit = chosen_limit(&settings);
    let mut node = Node {
        runner,
        control,
        store,
        screen,
        started: node::Started {
            reset,
            read_ms,
            loaded_ms,
        },
        small_coin: views::SmallCoin::new(),
        commands,
        network,
        button: Input::new(
            peripherals.GPIO0,
            InputConfig::default().with_pull(Pull::Up),
        ),
        view: View::from_name(&settings.view).unwrap_or(View::Basic),
        language: Language::from_name(&settings.language).unwrap_or(Language::English),
        mining: true,
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
    if updated {
        node.note(0, views::Note::Updated(control::BUILD));
    }
    loop {
        if node.turn() {
            yield_now().await;
        } else {
            Timer::after_millis(5).await;
        }
    }
}
