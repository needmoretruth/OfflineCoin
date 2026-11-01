//! The USB cable. A line starting with `#` is a hex-encoded node message, so a computer can join
//! the network through a board. Any other line is a typed command.
use alloc::collections::VecDeque;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::sync::atomic::{AtomicU32, Ordering};
use critical_section::Mutex;
use esp_hal::handler;
use esp_hal::usb::usb_serial_jtag::UsbSerialJtagRx;
use esp_hal::Blocking;
use esp_println::println;
use olc_node::{Link, Target};

const MAX_LINE: usize = 20_000;

/// The clock when a line was last in progress, which keeps mining out of its way: the cable's
/// buffer is small, so a long line would crawl at one buffer per mining batch.
pub static LINE_IN_PROGRESS_MS: AtomicU32 = AtomicU32::new(0);

/// Bytes wait here for the main loop. They come out of the cable's buffer only when a whole packet
/// is in: reading while one arrives loses its first bytes, which damaged about one line in ten.
const WAITING_BYTES: usize = 8 * 1024;
const TAKEN_AT_ONCE: usize = 256;

struct Receiver {
    rx: UsbSerialJtagRx<'static, Blocking>,
    bytes: [u8; WAITING_BYTES],
    start: usize,
    size: usize,
    /// The store filled up, so a packet stays in the cable's buffer and the computer waits.
    held_back: bool,
}

impl Receiver {
    fn pull(&mut self) {
        while self.size < WAITING_BYTES {
            let Ok(byte) = self.rx.read_byte() else {
                return;
            };
            self.bytes[(self.start + self.size) % WAITING_BYTES] = byte;
            self.size += 1;
        }
        self.held_back = true;
    }

    fn take(&mut self, out: &mut [u8; TAKEN_AT_ONCE]) -> usize {
        let count = self.size.min(TAKEN_AT_ONCE);
        for byte in &mut out[..count] {
            *byte = self.bytes[self.start];
            self.start = (self.start + 1) % WAITING_BYTES;
        }
        self.size -= count;
        if self.held_back {
            self.held_back = false;
            self.pull();
        }
        count
    }
}

static RECEIVER: Mutex<RefCell<Option<Receiver>>> = Mutex::new(RefCell::new(None));

#[handler]
pub fn on_usb_packet() {
    critical_section::with(|cs| {
        if let Some(receiver) = RECEIVER.borrow_ref_mut(cs).as_mut() {
            receiver.rx.reset_rx_packet_recv_interrupt();
            receiver.pull();
        }
    });
}

/// Typed lines waiting for the main loop.
pub type Commands = Rc<RefCell<VecDeque<String>>>;

pub struct UsbLink {
    line: Vec<u8>,
    messages: VecDeque<Vec<u8>>,
    commands: Commands,
    /// Nothing is sent to the cable until a computer has spoken on it.
    bridged: bool,
}

// SAFETY: the firmware has one task and `UsbLink` never leaves it.
unsafe impl Send for UsbLink {}

impl UsbLink {
    pub fn new(mut rx: UsbSerialJtagRx<'static, Blocking>) -> (Self, Commands) {
        // A computer may have been writing all through the start. The interrupt must not fire
        // before the receiver is in place: nothing would clear it and the chip would stay in it.
        critical_section::with(|cs| {
            rx.listen_rx_packet_recv_interrupt();
            let receiver = Receiver {
                rx,
                bytes: [0; WAITING_BYTES],
                start: 0,
                size: 0,
                held_back: false,
            };
            RECEIVER.replace(cs, Some(receiver));
        });
        let commands = Commands::default();
        let link = Self {
            line: Vec::new(),
            messages: VecDeque::new(),
            commands: commands.clone(),
            bridged: false,
        };
        (link, commands)
    }

    fn drain(&mut self) {
        let mut taken = [0u8; TAKEN_AT_ONCE];
        loop {
            let count = critical_section::with(|cs| {
                let mut receiver = RECEIVER.borrow_ref_mut(cs);
                receiver
                    .as_mut()
                    .map_or(0, |receiver| receiver.take(&mut taken))
            });
            if count == 0 {
                break;
            }
            for byte in &taken[..count] {
                match *byte {
                    b'\n' | b'\r' => self.finish_line(),
                    _ if self.line.len() < MAX_LINE => self.line.push(*byte),
                    _ => {}
                }
            }
        }
        if !self.line.is_empty() {
            LINE_IN_PROGRESS_MS.store(crate::node::device_ms() as u32, Ordering::Relaxed);
        }
    }

    fn finish_line(&mut self) {
        let line = core::mem::take(&mut self.line);
        match line.split_first() {
            Some((b'#', hex)) => {
                self.bridged = true;
                if let Some(message) = decode_hex(hex) {
                    self.messages.push_back(message);
                }
            }
            Some(_) => {
                if let Ok(text) = core::str::from_utf8(&line) {
                    self.commands.borrow_mut().push_back(text.into());
                }
            }
            None => {}
        }
    }
}

fn decode_hex(hex: &[u8]) -> Option<Vec<u8>> {
    let digit = |byte: u8| (byte as char).to_digit(16).map(|value| value as u8);
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    hex.chunks(2)
        .map(|pair| Some(digit(pair[0])? << 4 | digit(pair[1])?))
        .collect()
}

impl Link for UsbLink {
    fn send(&mut self, target: Target, message: &[u8]) {
        if !self.bridged || target == Target::AllExcept(0) {
            return;
        }
        let mut line = String::with_capacity(message.len() * 2 + 1);
        line.push('#');
        for byte in message {
            let _ = core::fmt::Write::write_fmt(&mut line, format_args!("{byte:02x}"));
        }
        println!("{line}");
    }

    fn receive(&mut self) -> Option<(u32, Vec<u8>)> {
        self.drain();
        self.messages.pop_front().map(|message| (0, message))
    }
}
