//! The one flash chip, shared by the board's settings and the chain's stores.
use alloc::boxed::Box;
use core::cell::RefCell;
use critical_section::Mutex;
use esp_storage::FlashStorage;
use olc_core::medium::{Keep, Medium, SECTOR};

static FLASH: Mutex<RefCell<Option<FlashStorage<'static>>>> = Mutex::new(RefCell::new(None));

pub fn give(flash: FlashStorage<'static>) {
    critical_section::with(|lock| *FLASH.borrow_ref_mut(lock) = Some(flash));
}

/// Moves the driver out for the length of the job, so a slow erase holds no lock here.
pub fn with<R>(job: impl FnOnce(&mut FlashStorage<'static>) -> R) -> Option<R> {
    let mut flash = critical_section::with(|lock| FLASH.borrow_ref_mut(lock).take())?;
    let result = job(&mut flash);
    critical_section::with(|lock| *FLASH.borrow_ref_mut(lock) = Some(flash));
    Some(result)
}

pub fn read(at: u32, bytes: &mut [u8]) -> bool {
    with(|flash| flash.read(at, bytes).is_ok()).unwrap_or(false)
}

/// Writes through a read and an erase of the sector, for settings that fit one.
pub fn replace(at: u32, bytes: &[u8]) -> bool {
    with(|flash| flash.write(at, bytes).is_ok()).unwrap_or(false)
}

/// Writes onto erased flash, which never touches the rest of the sector.
pub fn program(at: u32, bytes: &[u8]) -> bool {
    with(|flash| flash.write_nor(at, bytes).is_ok()).unwrap_or(false)
}

pub fn erase(at: u32) -> bool {
    with(|flash| flash.erase(at, at + SECTOR as u32).is_ok()).unwrap_or(false)
}

/// A stretch of flash the chain writes in a ring.
pub struct Region {
    pub start: u32,
    pub size: u32,
}

impl Region {
    fn place(&self, at: u64) -> u32 {
        (at % u64::from(self.size)) as u32
    }

    /// The bytes that fit before the end of the region, then those that fold to its start.
    fn split(&self, at: u64, length: usize) -> (u32, usize) {
        let place = self.place(at);
        (place, length.min((self.size - place) as usize))
    }
}

impl Medium for Region {
    fn capacity(&self) -> u64 {
        u64::from(self.size)
    }

    fn read(&self, at: u64, bytes: &mut [u8]) -> bool {
        let (place, first) = self.split(at, bytes.len());
        let (before, after) = bytes.split_at_mut(first);
        read(self.start + place, before) && (after.is_empty() || read(self.start, after))
    }

    fn write(&mut self, at: u64, bytes: &[u8]) -> bool {
        let (place, first) = self.split(at, bytes.len());
        let (before, after) = bytes.split_at(first);
        program(self.start + place, before) && (after.is_empty() || program(self.start, after))
    }

    fn erase(&mut self, at: u64) -> bool {
        erase(self.start + self.place(at))
    }
}

pub fn keep(log: Region, states: [Region; 2], recent: Option<u64>) -> Keep {
    let [first, second] = states;
    Keep {
        log: Box::new(log),
        states: [Box::new(first), Box::new(second)],
        shelf: None,
        recent,
    }
}
