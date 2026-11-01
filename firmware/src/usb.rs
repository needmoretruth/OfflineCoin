//! The USB cable. A line starting with `#` is a hex-encoded node message, so a computer can join
//! the network through a board. Any other line is a typed command.
use alloc::collections::VecDeque;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::sync::atomic::{AtomicU32, Ordering};
use esp_hal::usb::usb_serial_jtag::UsbSerialJtagRx;
use esp_hal::Blocking;
use esp_println::println;
use olc_node::{Link, Target};

const MAX_LINE: usize = 20_000;

/// The clock when a line was last in progress, which keeps mining out of its way: the cable's
/// buffer is small, so a long line would crawl at one buffer per mining batch.
pub static LINE_IN_PROGRESS_MS: AtomicU32 = AtomicU32::new(0);

/// Typed lines waiting for the main loop.
pub type Commands = Rc<RefCell<VecDeque<String>>>;

pub struct UsbLink {
    rx: UsbSerialJtagRx<'static, Blocking>,
    line: Vec<u8>,
    messages: VecDeque<Vec<u8>>,
    commands: Commands,
    /// Nothing is sent to the cable until a computer has spoken on it.
    bridged: bool,
}

// SAFETY: the firmware has one task and `UsbLink` never leaves it.
unsafe impl Send for UsbLink {}

impl UsbLink {
    pub fn new(rx: UsbSerialJtagRx<'static, Blocking>) -> (Self, Commands) {
        let commands = Commands::default();
        let link = Self {
            rx,
            line: Vec::new(),
            messages: VecDeque::new(),
            commands: commands.clone(),
            bridged: false,
        };
        (link, commands)
    }

    fn drain(&mut self) {
        while let Ok(byte) = self.rx.read_byte() {
            match byte {
                b'\n' | b'\r' => self.finish_line(),
                _ if self.line.len() < MAX_LINE => self.line.push(byte),
                _ => {}
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
