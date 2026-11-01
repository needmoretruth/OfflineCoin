//! One message per line: `#` followed by hex. Every other line is the board's log.
use olc_node::{Link, Medium, Target};
use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread;

const READ_SIZE: usize = 4096;
/// A board that is printing can lose up to three bytes where a transfer begins, so every line
/// starts with bytes it can spare. Empty lines mean nothing to a board.
const LINE_START: &str = "\n\n\n\n";
/// Writes waiting for the board. A board that is starting takes nothing for a minute, and what
/// it missed by then is old news.
const WRITES_WAITING: usize = 64;

pub struct SerialLink {
    writes: SyncSender<Vec<u8>>,
    incoming: Receiver<Vec<u8>>,
    closed: Arc<AtomicBool>,
}

impl Drop for SerialLink {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::Relaxed);
    }
}

fn read_lines(mut reader: File, closed: &AtomicBool, mut on_line: impl FnMut(&[u8]) -> bool) {
    let mut buffer = [0u8; READ_SIZE];
    let mut line = Vec::new();
    while !closed.load(Ordering::Relaxed) {
        let Ok(size) = reader.read(&mut buffer) else {
            return;
        };
        for byte in &buffer[..size] {
            if *byte != b'\n' {
                line.push(*byte);
            } else if on_line(&line) {
                line.clear();
            } else {
                return;
            }
        }
    }
}

impl SerialLink {
    /// Opens a serial device such as `/dev/ttyACM0`. `on_log` receives the other lines.
    pub fn open(path: &str, on_log: impl Fn(&str) + Send + 'static) -> std::io::Result<Self> {
        // A read waits at most 0.2 s, so closing does not hang on a quiet board.
        Command::new("stty")
            .args([
                "-F", path, "115200", "raw", "-echo", "-hupcl", "min", "0", "time", "2",
            ])
            .status()?;
        let mut writer = OpenOptions::new().read(true).write(true).open(path)?;
        let reader = writer.try_clone()?;
        // A board that takes nothing blocks the write, and a node must not wait for one board.
        let (writes, waiting) = sync_channel::<Vec<u8>>(WRITES_WAITING);
        thread::spawn(move || {
            while let Ok(bytes) = waiting.recv() {
                if writer.write_all(&bytes).is_err() {
                    return;
                }
            }
        });
        let (sender, incoming) = channel();
        let closed = Arc::new(AtomicBool::new(false));
        let reader_closed = closed.clone();
        thread::spawn(move || {
            read_lines(reader, &reader_closed, |line| {
                // Bytes, not text: a board that restarts prints garbage, and that must not end this.
                let line = String::from_utf8_lossy(line);
                match line.trim_end().strip_prefix('#').map(decode_hex) {
                    Some(Some(message)) => return sender.send(message).is_ok(),
                    Some(None) => {}
                    None => on_log(line.trim_end()),
                }
                true
            });
        });
        Ok(Self {
            writes,
            incoming,
            closed,
        })
    }

    /// Setting this stops the reader, which frees the port for a flashing tool.
    pub fn closer(&self) -> Arc<AtomicBool> {
        self.closed.clone()
    }

    fn write(&self, bytes: Vec<u8>) -> std::io::Result<()> {
        self.writes.try_send(bytes).map_err(|error| match error {
            TrySendError::Full(_) => ErrorKind::WouldBlock.into(),
            TrySendError::Disconnected(_) => ErrorKind::BrokenPipe.into(),
        })
    }

    /// Sends a typed command, as a person at a serial terminal would.
    pub fn send_line(&mut self, line: &str) -> std::io::Result<()> {
        self.write(format!("{LINE_START}{line}\n").into_bytes())
    }
}

fn decode_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    (0..text.len() / 2)
        .map(|index| u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).ok())
        .collect()
}

impl Link for SerialLink {
    fn medium(&self) -> Medium {
        Medium::Wire
    }

    fn send(&mut self, target: Target, message: &[u8]) {
        if target == Target::AllExcept(0) {
            return;
        }
        let mut line = String::with_capacity(message.len() * 2 + 2 + LINE_START.len());
        line.push_str(LINE_START);
        line.push('#');
        for byte in message {
            line.push_str(&format!("{byte:02x}"));
        }
        line.push('\n');
        if !self.closed.load(Ordering::Relaxed) {
            // Whatever is lost here, the protocols above send again.
            let _ = self.write(line.into_bytes());
        }
    }

    fn receive(&mut self) -> Option<(u32, Vec<u8>)> {
        self.incoming.try_recv().ok().map(|message| (0, message))
    }
}
