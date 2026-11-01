//! Keeps a frequent small write from wearing out one flash sector. Records go one after another
//! into erased flash, so a sector is erased once per many records instead of once per record.
use alloc::vec::Vec;

/// What the rings need from flash. `program` only clears bits.
pub trait Flash {
    fn read(&mut self, at: u32, bytes: &mut [u8]) -> bool;
    fn program(&mut self, at: u32, bytes: &[u8]) -> bool;
    fn erase(&mut self, sector: u32) -> bool;
}

pub const SECTOR: u32 = 0x1000;
const ERASED: u8 = 0xFF;
const LONGEST_SLOT: usize = 256;

/// Records of `slot` bytes in `sectors` sectors from `start`. With one sector, the newest record
/// is gone between the erase and the next write, so the caller keeps it in memory.
#[derive(Clone, Copy)]
pub struct Ring {
    pub start: u32,
    pub sectors: u32,
    pub slot: u32,
}

impl Ring {
    fn slots(&self) -> u32 {
        self.sectors * SECTOR / self.slot
    }

    fn at(&self, index: u32) -> u32 {
        self.start + index * self.slot
    }

    fn is_free(&self, flash: &mut impl Flash, index: u32) -> bool {
        let mut bytes = [0u8; LONGEST_SLOT];
        let bytes = &mut bytes[..self.slot as usize];
        flash.read(self.at(index), bytes) && bytes.iter().all(|byte| *byte == ERASED)
    }

    /// The newest record's index. Slots fill in order and the sector ahead is kept erased, so
    /// the newest is the used slot in front of the free ones.
    fn newest_index(&self, flash: &mut impl Flash) -> Option<u32> {
        let slots = self.slots();
        let mut first_free_after_used = None;
        let mut any_used = false;
        let mut was_used = !self.is_free(flash, slots - 1);
        for index in 0..slots {
            let used = !self.is_free(flash, index);
            if was_used && !used && first_free_after_used.is_none() {
                first_free_after_used = Some(index);
            }
            any_used |= used;
            was_used = used;
        }
        match first_free_after_used {
            Some(free) => Some((free + slots - 1) % slots),
            None => any_used.then_some(slots - 1),
        }
    }

    pub fn newest(&self, flash: &mut impl Flash) -> Option<u32> {
        self.newest_index(flash).map(|index| self.at(index))
    }

    /// Up to `count` records' places, the newest first.
    pub fn newest_first(&self, flash: &mut impl Flash, count: usize) -> Vec<u32> {
        let mut places = Vec::new();
        let Some(newest) = self.newest_index(flash) else {
            return places;
        };
        let slots = self.slots();
        for back in 0..(count as u32).min(slots) {
            let index = (newest + slots - back) % slots;
            if self.is_free(flash, index) {
                break;
            }
            places.push(self.at(index));
        }
        places
    }

    /// Where the next record goes. Erases only when it has run out of room.
    pub fn next(&self, flash: &mut impl Flash) -> u32 {
        let slots = self.slots();
        let index = self.newest_index(flash).map_or(0, |newest| newest + 1) % slots;
        if !self.is_free(flash, index) {
            flash.erase(self.at(index) / SECTOR * SECTOR);
        }
        self.at(index)
    }

    /// Call after writing a record. With more than one sector, the one ahead is erased as soon
    /// as this one is full, so the newest record can always be told.
    pub fn written(&self, flash: &mut impl Flash, at: u32) {
        let end = at + self.slot;
        if self.sectors > 1 && end % SECTOR == 0 {
            let last = self.start + self.sectors * SECTOR;
            flash.erase(if end == last { self.start } else { end });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::collections::BTreeMap;
    use alloc::vec;

    /// Erases the flash chips are rated for, per sector.
    const RATED_ERASES: u64 = 100_000;
    /// At thirty-second blocks that is one erase of a sector every sixteen minutes.
    const YEARS: u64 = 3;
    const DAY_S: u64 = 24 * 3600;

    const BASE: u32 = 0x1000;

    struct Fake {
        bytes: Vec<u8>,
        erases: BTreeMap<u32, u64>,
    }

    impl Fake {
        fn new(sectors: u32) -> Self {
            Self {
                bytes: vec![ERASED; ((sectors + 1) * SECTOR) as usize],
                erases: BTreeMap::new(),
            }
        }

        fn worst(&self) -> u64 {
            self.erases.values().copied().max().unwrap_or(0)
        }
    }

    impl Flash for Fake {
        fn read(&mut self, at: u32, bytes: &mut [u8]) -> bool {
            let at = at as usize;
            bytes.copy_from_slice(&self.bytes[at..at + bytes.len()]);
            true
        }

        fn program(&mut self, at: u32, bytes: &[u8]) -> bool {
            for (offset, byte) in bytes.iter().enumerate() {
                self.bytes[at as usize + offset] &= byte;
            }
            true
        }

        fn erase(&mut self, sector: u32) -> bool {
            assert_eq!(sector % SECTOR, 0);
            let at = sector as usize;
            self.bytes[at..at + SECTOR as usize].fill(ERASED);
            *self.erases.entry(sector).or_default() += 1;
            true
        }
    }

    fn append(ring: &Ring, flash: &mut Fake, record: &[u8]) -> u32 {
        let at = ring.next(flash);
        flash.program(at, record);
        ring.written(flash, at);
        at
    }

    /// Erases per sector in [`YEARS`], from counting them over `sample` records.
    fn over_the_years(erases: u64, records: u64, records_a_day: u64) -> u64 {
        erases * records_a_day * 365 * YEARS / records
    }

    #[test]
    fn records_read_back_newest_first_across_the_seam() {
        let ring = Ring {
            start: BASE,
            sectors: 2,
            slot: 4,
        };
        let mut flash = Fake::new(3);
        let count = 3 * 2048 + 7;
        for value in 0..count as u32 {
            append(&ring, &mut flash, &value.to_le_bytes());
            let mut bytes = [0u8; 4];
            let newest = ring.newest(&mut flash).unwrap();
            flash.read(newest, &mut bytes);
            assert_eq!(u32::from_le_bytes(bytes), value);
        }
        let places = ring.newest_first(&mut flash, 3);
        assert_eq!(places.len(), 3);
        assert_eq!(places[0], places[1] + 4);
    }

    #[test]
    fn a_record_saved_by_the_old_layout_is_the_newest() {
        let ring = Ring {
            start: BASE,
            sectors: 1,
            slot: 8,
        };
        let mut flash = Fake::new(2);
        flash.program(BASE, &41u64.to_le_bytes());
        assert_eq!(ring.newest(&mut flash), Some(BASE));
        assert_eq!(ring.next(&mut flash), BASE + 8);
    }

    #[test]
    fn a_full_single_sector_starts_over() {
        let ring = Ring {
            start: BASE,
            sectors: 1,
            slot: 256,
        };
        let mut flash = Fake::new(2);
        for value in 0..17u8 {
            append(&ring, &mut flash, &[value; 256]);
        }
        assert_eq!(flash.worst(), 1);
        assert_eq!(ring.newest(&mut flash), Some(BASE));
    }

    #[test]
    fn a_rewrite_in_place_would_fail_this_budget() {
        // The way the counter was saved before: one erase of one sector per order.
        let orders_a_day = 2_000;
        assert!(over_the_years(1, 1, orders_a_day) > RATED_ERASES);
    }

    #[test]
    fn the_order_counter_lasts_at_one_order_every_ten_seconds() {
        let ring = Ring {
            start: BASE,
            sectors: 1,
            slot: 8,
        };
        let mut flash = Fake::new(2);
        let records = 20_000u64;
        for value in 0..records {
            append(&ring, &mut flash, &value.to_le_bytes());
        }
        assert!(over_the_years(flash.worst(), records, DAY_S / 10) <= RATED_ERASES);
    }

    #[test]
    fn settings_last_at_a_hundred_changes_a_day() {
        let ring = Ring {
            start: BASE,
            sectors: 1,
            slot: 256,
        };
        let mut flash = Fake::new(2);
        let records = 2_000u64;
        for _ in 0..records {
            append(&ring, &mut flash, &[0x5A; 256]);
        }
        assert!(over_the_years(flash.worst(), records, 100) <= RATED_ERASES);
    }

    #[test]
    fn the_boot_log_lasts_through_a_three_second_restart_loop() {
        let ring = Ring {
            start: BASE,
            sectors: 2,
            slot: 4,
        };
        let mut flash = Fake::new(3);
        let records = 40_000u64;
        for value in 0..records {
            append(&ring, &mut flash, &(value as u32).to_le_bytes());
        }
        assert!(over_the_years(flash.worst(), records, DAY_S / 3) <= RATED_ERASES);
    }

    #[test]
    fn the_board_writes_its_frequent_records_through_a_ring() {
        let store = include_str!("../../../firmware/src/store.rs");
        for record in ["COUNTER: Ring", "SETTINGS: Ring", "BOOT_LOG: Ring"] {
            assert!(store.contains(record), "{record}");
        }
    }
}
