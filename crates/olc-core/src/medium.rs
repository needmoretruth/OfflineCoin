//! The socket for storage. A board plugs in its flash and a computer its files. Everything above
//! this trait is the same code on both, so what a test proves on a computer holds on a board.
use crate::shelf::Shelf;
use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;

/// Bytes wiped at once, as on NOR flash. Records never rely on anything larger.
pub const SECTOR: u64 = 4096;

/// Bytes that survive a power cut, addressed by a position that only ever grows. A medium with a
/// fixed size folds positions into itself and so overwrites the oldest bytes.
pub trait Medium: Send {
    /// The most bytes it holds at once.
    fn capacity(&self) -> u64;
    /// False when those bytes are gone or cannot be read.
    fn read(&self, at: u64, bytes: &mut [u8]) -> bool;
    /// Writes to bytes that were erased. `at` and the length are multiples of four.
    fn write(&mut self, at: u64, bytes: &[u8]) -> bool;
    /// Wipes the sector starting at `at`.
    fn erase(&mut self, at: u64) -> bool;
    /// The lowest position that may hold anything.
    fn lowest(&self) -> u64 {
        0
    }
    /// The position from which nothing was ever written, when the medium knows it without
    /// reading, as a computer's files do. A survey stops there instead of at the capacity.
    fn written_end(&self) -> Option<u64> {
        None
    }
    /// Bytes before `below` are not wanted any more, so a file may give them back.
    fn forget(&mut self, _below: u64) {}
    /// Returns once everything written is safe from a power cut.
    fn sync(&mut self) {}
}

/// A medium in memory: bits only fall on write and only rise on erase, as on flash. It holds a
/// chain as long as the process lives, and it is how a machine with no disk runs.
pub struct MemoryMedium {
    bytes: Vec<u8>,
    /// Folds positions when set.
    ring: Option<u64>,
}

impl MemoryMedium {
    pub fn growing() -> Self {
        Self {
            bytes: Vec::new(),
            ring: None,
        }
    }

    pub fn ring(capacity: u64) -> Self {
        Self {
            bytes: vec![0xFF; capacity as usize],
            ring: Some(capacity),
        }
    }

    fn place(&self, at: u64) -> usize {
        self.ring.map_or(at, |size| at % size) as usize
    }
}

impl Medium for MemoryMedium {
    fn capacity(&self) -> u64 {
        self.ring.unwrap_or(u64::MAX)
    }

    fn read(&self, at: u64, bytes: &mut [u8]) -> bool {
        for (step, byte) in bytes.iter_mut().enumerate() {
            let place = self.place(at + step as u64);
            *byte = self.bytes.get(place).copied().unwrap_or(0xFF);
        }
        true
    }

    fn write(&mut self, at: u64, bytes: &[u8]) -> bool {
        for (step, byte) in bytes.iter().enumerate() {
            let place = self.place(at + step as u64);
            if place >= self.bytes.len() {
                self.bytes.resize(place + 1, 0xFF);
            }
            self.bytes[place] &= byte;
        }
        true
    }

    fn erase(&mut self, at: u64) -> bool {
        for step in 0..SECTOR {
            let place = self.place(at + step);
            if let Some(byte) = self.bytes.get_mut(place) {
                *byte = 0xFF;
            }
        }
        true
    }
}

/// Everything a chain keeps outside memory.
pub struct Keep {
    pub log: Box<dyn Medium>,
    /// Two places for the saved state, so a cut while saving leaves the other one whole.
    pub states: [Box<dyn Medium>; 2],
    /// Where headers stay after their blocks go, if this machine has room for that.
    pub shelf: Option<Box<dyn Shelf>>,
    /// How many recent blocks to hold. `None` holds all that fit.
    pub recent: Option<u64>,
}

impl Keep {
    pub fn in_memory() -> Self {
        Self {
            log: Box::new(MemoryMedium::growing()),
            states: [
                Box::new(MemoryMedium::growing()),
                Box::new(MemoryMedium::growing()),
            ],
            shelf: None,
            recent: None,
        }
    }
}
