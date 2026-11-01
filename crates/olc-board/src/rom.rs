//! A board that powers up with its boot pin read low sits in the chip's built-in loader and runs
//! nothing of ours. On a cable it can still be told to restart, in the loader's own language.
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::process::Command;

const FRAME_END: u8 = 0xC0;
const ESCAPE: u8 = 0xDB;
const ESCAPED_END: u8 = 0xDC;
const ESCAPED_ESCAPE: u8 = 0xDD;

const SYNC: u8 = 0x08;
const WRITE_REGISTER: u8 = 0x09;
const READ_REGISTER: u8 = 0x0A;

const SYNC_TRIES: usize = 3;
const REPLY_BYTES: usize = 512;
/// The port gives up on a read after this many tenths of a second.
const READ_TENTHS: &str = "2";
/// A loader answers a sync several times. A running board prints other things.
const QUIET_READS: usize = 2;

const WATCHDOG_PROTECT: u32 = 0x6000_80B0;
const WATCHDOG_KEY: u32 = 0x50D8_3AA1;
const WATCHDOG_CONFIG: u32 = 0x6000_8098;
const WATCHDOG_TIMEOUT: u32 = 0x6000_809C;
const WATCHDOG_TICKS: u32 = 2000;
const WATCHDOG_ON: u32 = (1 << 31) | (5 << 28) | (1 << 8) | 2;
const STRAP_REGISTER: u32 = 0x6000_4038;

fn slip(bytes: &[u8]) -> Vec<u8> {
    let mut out = vec![FRAME_END];
    for byte in bytes {
        match *byte {
            FRAME_END => out.extend([ESCAPE, ESCAPED_END]),
            ESCAPE => out.extend([ESCAPE, ESCAPED_ESCAPE]),
            byte => out.push(byte),
        }
    }
    out.push(FRAME_END);
    out
}

fn unslip(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut escaped = false;
    for byte in bytes {
        match (escaped, *byte) {
            (true, ESCAPED_END) => out.push(FRAME_END),
            (true, ESCAPED_ESCAPE) => out.push(ESCAPE),
            (false, ESCAPE) => {}
            (_, byte) => out.push(byte),
        }
        escaped = !escaped && *byte == ESCAPE;
    }
    out
}

fn request(command: u8, data: &[u8]) -> Vec<u8> {
    let mut packet = vec![0, command];
    packet.extend((data.len() as u16).to_le_bytes());
    packet.extend(0u32.to_le_bytes());
    packet.extend(data);
    slip(&packet)
}

fn sync_request() -> Vec<u8> {
    let mut data = vec![0x07, 0x07, 0x12, 0x20];
    data.extend([0x55; 32]);
    request(SYNC, &data)
}

fn write_request(register: u32, value: u32) -> Vec<u8> {
    let mut data = Vec::with_capacity(16);
    data.extend(register.to_le_bytes());
    data.extend(value.to_le_bytes());
    data.extend(u32::MAX.to_le_bytes());
    data.extend(0u32.to_le_bytes());
    request(WRITE_REGISTER, &data)
}

/// The value in the loader's answer to `command`, if these bytes hold one.
fn answer(bytes: &[u8], command: u8) -> Option<u32> {
    bytes.split(|byte| *byte == FRAME_END).find_map(|frame| {
        let packet = unslip(frame);
        let value = packet.get(4..8)?;
        (packet.first() == Some(&1) && packet.get(1) == Some(&command))
            .then(|| u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
    })
}

struct Loader {
    port: File,
}

impl Loader {
    fn open(path: &str) -> Option<Self> {
        let settings = [
            "-F", path, "115200", "raw", "-echo", "-hupcl", "min", "0", "time",
        ];
        let set = Command::new("stty")
            .args(settings)
            .arg(READ_TENTHS)
            .status();
        if !set.is_ok_and(|status| status.success()) {
            return None;
        }
        let port = OpenOptions::new().read(true).write(true).open(path).ok()?;
        Some(Self { port })
    }

    fn ask(&mut self, message: &[u8], command: u8) -> Option<u32> {
        self.port.write_all(message).ok()?;
        let mut heard = Vec::new();
        let mut buffer = [0u8; REPLY_BYTES];
        for _ in 0..QUIET_READS {
            let size = self.port.read(&mut buffer).ok()?;
            heard.extend(&buffer[..size]);
            if let Some(value) = answer(&heard, command) {
                return Some(value);
            }
        }
        None
    }

    fn sync(&mut self) -> bool {
        (0..SYNC_TRIES).any(|_| self.ask(&sync_request(), SYNC).is_some())
    }

    fn write(&mut self, register: u32, value: u32) {
        self.ask(&write_request(register, value), WRITE_REGISTER);
    }
}

/// What the chip read from its boot pins, when the loader is what answers on this port.
pub fn pins_if_in_loader(path: &str) -> Option<u32> {
    let mut loader = Loader::open(path)?;
    if !loader.sync() {
        return None;
    }
    let register = STRAP_REGISTER.to_le_bytes();
    loader.ask(&request(READ_REGISTER, &register), READ_REGISTER)
}

/// Restarts a board sitting in the loader, the way a loader itself allows. Says whether it was
/// there. A running board takes the bytes for one bad line.
pub fn wake(path: &str) -> bool {
    let Some(mut loader) = Loader::open(path) else {
        return false;
    };
    if !loader.sync() {
        return false;
    }
    loader.write(WATCHDOG_PROTECT, WATCHDOG_KEY);
    loader.write(WATCHDOG_TIMEOUT, WATCHDOG_TICKS);
    loader.write(WATCHDOG_CONFIG, WATCHDOG_ON);
    loader.write(WATCHDOG_PROTECT, 0);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_that_mean_something_to_the_framing_are_escaped() {
        let bytes = [1, FRAME_END, 2, ESCAPE, 3];
        let framed = slip(&bytes);
        assert_eq!(framed.iter().filter(|byte| **byte == FRAME_END).count(), 2);
        assert_eq!(unslip(&framed[1..framed.len() - 1]), bytes);
    }

    #[test]
    fn a_register_write_is_what_the_loader_expects() {
        let framed = write_request(WATCHDOG_PROTECT, WATCHDOG_KEY);
        let packet = unslip(&framed[1..framed.len() - 1]);
        assert_eq!(&packet[..2], &[0, WRITE_REGISTER]);
        assert_eq!(u16::from_le_bytes([packet[2], packet[3]]), 16);
        assert_eq!(&packet[8..12], &WATCHDOG_PROTECT.to_le_bytes());
        assert_eq!(&packet[12..16], &WATCHDOG_KEY.to_le_bytes());
    }

    #[test]
    fn only_a_reply_to_the_question_counts() {
        let reply = slip(&[1, READ_REGISTER, 2, 0, 0x0A, 0, 0, 0, 0, 0]);
        assert_eq!(answer(&reply, READ_REGISTER), Some(0x0A));
        assert_eq!(answer(&reply, SYNC), None);
        assert_eq!(answer(b"status height=7\nmined 8\n", SYNC), None);
    }
}
