//! Jumper wires between two boards: GPIO 17, GPIO 18 and ground. The two signal wires can run
//! straight or crossed. One serial port talks on 17 and hears 18, another talks on 18 and hears
//! 17, and [`Hunt`] picks the one that reaches the far end. Pins only ever pull low, so two ends
//! talking on one wire do no harm. Bytes arrive in an interrupt, because mining can hold the main
//! loop for longer than the hardware's small buffer lasts.
use alloc::collections::VecDeque;
use core::cell::RefCell;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use critical_section::Mutex;
use esp_hal::gpio::interconnect::InputSignal;
use esp_hal::gpio::{DriveMode, Flex, OutputConfig, Pin, Pull};
use esp_hal::handler;
use esp_hal::peripherals::{UART1, UART2};
use esp_hal::rng::Rng;
use esp_hal::uart::{Config, Instance, RxConfig, Uart, UartInterrupt};
use esp_hal::Blocking;
use olc_node::line::{Hunt, LineLink, Pipe};

/// Fast enough for blocks, and slow enough for loose wires a hand long.
const BAUD: u32 = 460_800;
/// Half the hardware buffer, so the interrupt has time to answer.
const FIFO_THRESHOLD: u16 = 64;
const MOST_WAITING: usize = 16 * 1024;

struct Wires {
    /// One way each. Both stay ready, so swapping moves no pin.
    ways: [Uart<'static, Blocking>; 2],
    using: usize,
    arrived: VecDeque<u8>,
}

/// Lines spoiled on the way, since the board started.
static SPOILED: AtomicU32 = AtomicU32::new(0);

static SWAPS: AtomicU32 = AtomicU32::new(0);
static WAY: AtomicU32 = AtomicU32::new(0);
static FOUND_AFTER_MS: AtomicU32 = AtomicU32::new(0);
static WANT_SWAP: AtomicBool = AtomicBool::new(false);

/// What `wires` prints.
pub fn report() -> alloc::string::String {
    alloc::format!(
        "wires talk={} spoiled={} swaps={} found_ms={}",
        if WAY.load(Ordering::Relaxed) == 0 { 17 } else { 18 },
        SPOILED.load(Ordering::Relaxed),
        SWAPS.load(Ordering::Relaxed),
        FOUND_AFTER_MS.load(Ordering::Relaxed),
    )
}

/// Leaves the link in use, to prove the search finds it again.
pub fn swap() {
    WANT_SWAP.store(true, Ordering::Relaxed);
}

static WIRES: Mutex<RefCell<Option<Wires>>> = Mutex::new(RefCell::new(None));

#[handler]
fn on_uart() {
    critical_section::with(|cs| {
        let mut wires = WIRES.borrow_ref_mut(cs);
        let Some(wires) = wires.as_mut() else {
            return;
        };
        let mut bytes = [0u8; 128];
        for way in 0..wires.ways.len() {
            while let Ok(size @ 1..) = wires.ways[way].read_buffered(&mut bytes) {
                if way == wires.using {
                    let room = MOST_WAITING.saturating_sub(wires.arrived.len());
                    wires.arrived.extend(&bytes[..size.min(room)]);
                }
            }
            wires.ways[way].clear_interrupts(UartInterrupt::RxFifoFull | UartInterrupt::RxTimeout);
        }
    });
}

/// The wires as the node sees them. Sending waits in `waiting` so a block never stalls mining.
pub struct Pins {
    waiting: VecDeque<u8>,
    hunt: Hunt,
    rng: Rng,
    /// How long the last search took, for `wires`.
    hunting_from_ms: u64,
}

impl Pipe for Pins {
    fn spoiled_line(&mut self) {
        SPOILED.fetch_add(1, Ordering::Relaxed);
    }

    fn heard_line(&mut self) {
        let now = crate::node::device_ms();
        if !self.hunt.linked(now) {
            FOUND_AFTER_MS.store((now - self.hunting_from_ms) as u32, Ordering::Relaxed);
        }
        self.hunt.heard(now);
    }

    fn write(&mut self, bytes: &[u8]) {
        if self.waiting.len() + bytes.len() <= MOST_WAITING {
            self.waiting.extend(bytes);
        }
    }

    fn read(&mut self, into: &mut [u8]) -> usize {
        let now = crate::node::device_ms();
        if self.hunt.linked(now) {
            self.hunting_from_ms = now;
        }
        if WANT_SWAP.swap(false, Ordering::Relaxed) {
            self.hunt.force_swap(now);
        }
        self.hunt.turn(now, self.rng.random());
        SWAPS.store(self.hunt.swaps, Ordering::Relaxed);
        WAY.store(self.hunt.way as u32, Ordering::Relaxed);
        critical_section::with(|cs| {
            let mut wires = WIRES.borrow_ref_mut(cs);
            let Some(wires) = wires.as_mut() else {
                return 0;
            };
            wires.using = usize::from(self.hunt.way);
            let using = wires.using;
            while wires.ways[using].write_ready() {
                let Some(byte) = self.waiting.pop_front() else {
                    break;
                };
                if wires.ways[using].write(&[byte]).is_err() {
                    break;
                }
            }
            let size = wires.arrived.len().min(into.len());
            for (to, from) in into.iter_mut().zip(wires.arrived.drain(..size)) {
                *to = from;
            }
            size
        })
    }
}

/// Changes the speed, to find the fastest a pair of wires carries clean. Both ends must match.
pub fn set_speed(baud: u32) -> bool {
    critical_section::with(|cs| {
        let mut wires = WIRES.borrow_ref_mut(cs);
        let Some(wires) = wires.as_mut() else {
            return false;
        };
        wires.ways.iter_mut().all(|way| way.apply_config(&config_at(baud)).is_ok())
    })
}

fn config_at(baud: u32) -> Config {
    Config::default()
        .with_baudrate(baud)
        .with_rx(RxConfig::default().with_fifo_full_threshold(FIFO_THRESHOLD))
}

fn one_way(device: impl Instance + 'static, talk: Flex<'static>, hear: InputSignal<'static>) -> Uart<'static, Blocking> {
    let (_, talk) = talk.split();
    // Frozen, or the driver sets the pin to push and pull. Each pin plays two roles, so neither
    // may take the shortcut that bypasses the pin matrix.
    let mut uart = Uart::new(device, config_at(BAUD))
        .expect("the serial settings are valid")
        .with_tx(talk.with_gpio_matrix_forced(true).freeze())
        .with_rx(hear.with_gpio_matrix_forced(true));
    uart.set_interrupt_handler(on_uart);
    uart.listen(UartInterrupt::RxFifoFull | UartInterrupt::RxTimeout);
    uart
}

fn open_drain(pin: impl Pin + 'static) -> Flex<'static> {
    let mut flex = Flex::new(pin);
    flex.apply_output_config(&OutputConfig::default().with_drive_mode(DriveMode::OpenDrain).with_pull(Pull::Up));
    flex.set_high();
    flex.set_output_enable(true);
    flex
}

/// Starts listening on the pins. With nothing attached this costs two quiet interrupt sources.
pub fn start(
    first: UART1<'static>,
    second: UART2<'static>,
    pin_a: impl Pin + 'static,
    pin_b: impl Pin + 'static,
) -> LineLink<Pins> {
    let (a, b) = (open_drain(pin_a), open_drain(pin_b));
    let (hear_a, hear_b) = (a.peripheral_input(), b.peripheral_input());
    let ways = [one_way(first, a, hear_b), one_way(second, b, hear_a)];
    let rng = Rng::new();
    let hunt = Hunt::new(rng.random());
    critical_section::with(|cs| {
        *WIRES.borrow_ref_mut(cs) = Some(Wires {
            ways,
            using: usize::from(hunt.way),
            arrived: VecDeque::with_capacity(1024),
        });
    });
    LineLink::new(Pins { waiting: VecDeque::new(), hunt, rng, hunting_from_ms: 0 })
}
