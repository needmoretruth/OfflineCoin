//! Jumper wires between two boards: GPIO 17 to the other board's GPIO 18, GPIO 18 to its GPIO 17,
//! and ground to ground. Bytes arrive in an interrupt, because mining can hold the main loop for
//! longer than the hardware's small buffer lasts.
use alloc::collections::VecDeque;
use core::cell::RefCell;
use core::sync::atomic::{AtomicU32, Ordering};
use critical_section::Mutex;
use esp_hal::gpio::interconnect::{PeripheralInput, PeripheralOutput};
use esp_hal::handler;
use esp_hal::peripherals::UART1;
use esp_hal::uart::{Config, RxConfig, Uart, UartInterrupt};
use esp_hal::Blocking;
use olc_node::line::{LineLink, Pipe};

/// Fast enough for blocks, and slow enough for loose wires a hand long.
const BAUD: u32 = 460_800;
/// Half the hardware buffer, so the interrupt has time to answer.
const FIFO_THRESHOLD: u16 = 64;
const MOST_WAITING: usize = 16 * 1024;

struct Wires {
    uart: Uart<'static, Blocking>,
    arrived: VecDeque<u8>,
}

/// Lines spoiled on the way, since the board started.
static SPOILED: AtomicU32 = AtomicU32::new(0);

pub fn spoiled_lines() -> u32 {
    SPOILED.load(Ordering::Relaxed)
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
        while let Ok(size @ 1..) = wires.uart.read_buffered(&mut bytes) {
            let room = MOST_WAITING.saturating_sub(wires.arrived.len());
            wires.arrived.extend(&bytes[..size.min(room)]);
        }
        wires.uart.clear_interrupts(UartInterrupt::RxFifoFull | UartInterrupt::RxTimeout);
    });
}

/// The wires as the node sees them. Sending waits in `waiting` so a block never stalls mining.
pub struct Pins {
    waiting: VecDeque<u8>,
}

impl Pipe for Pins {
    fn spoiled_line(&mut self) {
        SPOILED.fetch_add(1, Ordering::Relaxed);
    }

    fn write(&mut self, bytes: &[u8]) {
        if self.waiting.len() + bytes.len() <= MOST_WAITING {
            self.waiting.extend(bytes);
        }
    }

    fn read(&mut self, into: &mut [u8]) -> usize {
        critical_section::with(|cs| {
            let mut wires = WIRES.borrow_ref_mut(cs);
            let Some(wires) = wires.as_mut() else {
                return 0;
            };
            while wires.uart.write_ready() {
                let Some(byte) = self.waiting.pop_front() else {
                    break;
                };
                if wires.uart.write(&[byte]).is_err() {
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
        wires.uart.apply_config(&config_at(baud)).is_ok()
    })
}

fn config_at(baud: u32) -> Config {
    Config::default()
        .with_baudrate(baud)
        .with_rx(RxConfig::default().with_fifo_full_threshold(FIFO_THRESHOLD))
}

/// Starts listening on the pins. With nothing attached this costs one quiet interrupt source.
pub fn start(
    device: UART1<'static>,
    tx: impl PeripheralOutput<'static>,
    rx: impl PeripheralInput<'static>,
) -> LineLink<Pins> {
    let mut uart = Uart::new(device, config_at(BAUD))
        .expect("the serial settings are valid")
        .with_tx(tx)
        .with_rx(rx);
    uart.set_interrupt_handler(on_uart);
    uart.listen(UartInterrupt::RxFifoFull | UartInterrupt::RxTimeout);
    critical_section::with(|cs| {
        *WIRES.borrow_ref_mut(cs) = Some(Wires { uart, arrived: VecDeque::with_capacity(1024) });
    });
    LineLink::new(Pins { waiting: VecDeque::new() })
}
