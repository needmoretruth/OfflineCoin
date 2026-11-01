//! One message per line: `#` followed by hex. Every other line is the board's log.
use olc_node::{Link, Target};
use olc_platform::platform;
use serial2::{SerialPort, Settings};
use std::io::ErrorKind;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{
    channel, sync_channel, Receiver, RecvTimeoutError, SyncSender, TrySendError,
};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const READ_SIZE: usize = 4096;
const BAUD: u32 = 115_200;
/// A read or a write waits at most this, so closing does not hang on a quiet board.
const WAIT: Duration = Duration::from_millis(200);
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

/// Opens a serial device, raw, in the state a board expects of a terminal.
pub fn open_port(path: &str) -> std::io::Result<SerialPort> {
    let mut port = SerialPort::open(path, |mut settings: Settings| {
        settings.set_raw();
        settings.set_baud_rate(BAUD)?;
        platform().keep_lines_on_close(&mut settings);
        Ok(settings)
    })?;
    port.set_read_timeout(WAIT)?;
    port.set_write_timeout(WAIT)?;
    // In this order, since the line that restarts a board is RTS up with DTR down.
    let _ = port.set_dtr(true);
    let _ = port.set_rts(true);
    Ok(port)
}

/// Whether a device of this name is here now. Names like `COM3` are no files.
pub fn port_is_here(path: &str) -> bool {
    Path::new(path).exists()
        || SerialPort::available_ports().is_ok_and(|ports| {
            ports.iter().any(|port| port.as_os_str() == path)
        })
}

fn is_only_waiting(kind: ErrorKind) -> bool {
    matches!(kind, ErrorKind::TimedOut | ErrorKind::WouldBlock | ErrorKind::Interrupted)
}

/// A board that takes nothing holds this up until it does, or until the link closes.
fn write_out(port: &SerialPort, mut bytes: &[u8], closed: &AtomicBool) -> bool {
    while !bytes.is_empty() {
        if closed.load(Ordering::Relaxed) {
            return false;
        }
        match port.write(bytes) {
            Ok(0) => return false,
            Ok(size) => bytes = &bytes[size..],
            Err(error) if is_only_waiting(error.kind()) => {}
            Err(_) => return false,
        }
    }
    true
}

fn read_lines(reader: &SerialPort, closed: &AtomicBool, mut on_line: impl FnMut(&[u8]) -> bool) {
    let mut buffer = [0u8; READ_SIZE];
    let mut line = Vec::new();
    while !closed.load(Ordering::Relaxed) {
        let size = match reader.read(&mut buffer) {
            Ok(0) => return,
            Ok(size) => size,
            Err(error) if is_only_waiting(error.kind()) => continue,
            Err(_) => return,
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
    /// Opens a serial device. `on_log` receives the other lines.
    pub fn open(path: &str, on_log: impl Fn(&str) + Send + 'static) -> std::io::Result<Self> {
        let port = Arc::new(open_port(path)?);
        let closed = Arc::new(AtomicBool::new(false));
        let (writer, writer_closed) = (port.clone(), closed.clone());
        // A board that takes nothing blocks the write, and a node must not wait for one board.
        let (writes, waiting) = sync_channel::<Vec<u8>>(WRITES_WAITING);
        thread::spawn(move || loop {
            // Waits a little at a time, since Windows lets nobody else open a port still held.
            match waiting.recv_timeout(WAIT) {
                Ok(bytes) if write_out(&writer, &bytes, &writer_closed) => {}
                Ok(_) | Err(RecvTimeoutError::Disconnected) => return,
                Err(RecvTimeoutError::Timeout) if writer_closed.load(Ordering::Relaxed) => return,
                Err(RecvTimeoutError::Timeout) => {}
            }
        });
        let (sender, incoming) = channel();
        let reader_closed = closed.clone();
        thread::spawn(move || {
            read_lines(&port, &reader_closed, |line| {
                // Bytes, not text: a board that restarts prints garbage, and that must not end this.
                let line = String::from_utf8_lossy(line);
                match line.trim_end().strip_prefix('#').map(decode_hex) {
                    Some(Some(message)) => return sender.send(message).is_ok(),
                    Some(None) => {}
                    None => on_log(line.trim_end()),
                }
                true
            });
            // Whatever ended the reading, this link hears nothing any more.
            reader_closed.store(true, Ordering::Relaxed);
        });
        Ok(Self {
            writes,
            incoming,
            closed,
        })
    }

    /// Setting this stops the reader, which frees the port for a flashing tool. It also sets
    /// itself when the device goes away.
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

    /// A way to type on this link after the node took it over.
    pub fn typing(&self) -> Typing {
        Typing(self.writes.clone())
    }
}

/// Types lines on a link that belongs to a node.
#[derive(Clone)]
pub struct Typing(SyncSender<Vec<u8>>);

impl Typing {
    /// False when the link has gone, or the board has stopped taking anything.
    pub fn send(&self, line: &str) -> bool {
        self.0.try_send(format!("{LINE_START}{line}\n").into_bytes()).is_ok()
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
