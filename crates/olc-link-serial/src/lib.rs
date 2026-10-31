//! One message per line: `#` followed by hex. Every other line is the board's log.
use olc_node::{Link, Target};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::process::Command;
use std::sync::mpsc::{channel, Receiver};
use std::thread;

pub struct SerialLink {
    writer: File,
    incoming: Receiver<Vec<u8>>,
}

impl SerialLink {
    /// Opens a serial device such as `/dev/ttyACM0`. `on_log` receives the other lines.
    pub fn open(path: &str, on_log: impl Fn(&str) + Send + 'static) -> std::io::Result<Self> {
        Command::new("stty")
            .args(["-F", path, "115200", "raw", "-echo", "-hupcl"])
            .status()?;
        let writer = OpenOptions::new().read(true).write(true).open(path)?;
        let reader = writer.try_clone()?;
        let (sender, incoming) = channel();
        thread::spawn(move || {
            for line in BufReader::new(reader).lines().map_while(Result::ok) {
                match line.trim_end().strip_prefix('#').map(decode_hex) {
                    Some(Some(message)) => {
                        if sender.send(message).is_err() {
                            return;
                        }
                    }
                    Some(None) => {}
                    None => on_log(line.trim_end()),
                }
            }
        });
        Ok(Self { writer, incoming })
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
        let mut line = String::with_capacity(message.len() * 2 + 2);
        line.push('#');
        for byte in message {
            line.push_str(&format!("{byte:02x}"));
        }
        line.push('\n');
        let _ = self.writer.write_all(line.as_bytes());
    }

    fn receive(&mut self) -> Option<(u32, Vec<u8>)> {
        self.incoming.try_recv().ok().map(|message| (0, message))
    }
}
