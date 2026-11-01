//! The bytes of an ESP-NOW frame as a monitor-mode card sees and sends them.

pub const BROADCAST: [u8; 6] = [0xFF; 6];
/// What one frame carries, as on the boards.
pub const MOST_BODY: usize = 250;

const ACTION: u8 = 0xD0;
const VENDOR_ACTION: u8 = 127;
const ESPRESSIF: [u8; 3] = [0x18, 0xFE, 0x34];
const VENDOR_ELEMENT: u8 = 0xDD;
const ESP_NOW_TYPE: u8 = 4;
const VERSION: u8 = 1;
/// Frame control to the third address, and the sequence number.
const HEADER: usize = 24;
/// Category, Espressif's mark and four random bytes, which a receiver ignores.
const ACTION_START: usize = 8;
/// A radiotap header that asks for nothing: the driver picks the rate.
const PLAIN_RADIOTAP: [u8; 8] = [0, 0, 8, 0, 0, 0, 0, 0];

/// A frame ready for a monitor-mode card.
pub fn frame(to: &[u8; 6], from: &[u8; 6], body: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(PLAIN_RADIOTAP.len() + HEADER + ACTION_START + 7 + body.len());
    bytes.extend_from_slice(&PLAIN_RADIOTAP);
    bytes.extend_from_slice(&[ACTION, 0, 0, 0]);
    bytes.extend_from_slice(to);
    bytes.extend_from_slice(from);
    bytes.extend_from_slice(&BROADCAST);
    bytes.extend_from_slice(&[0, 0]);
    bytes.push(VENDOR_ACTION);
    bytes.extend_from_slice(&ESPRESSIF);
    bytes.extend_from_slice(&[0, 0, 0, 0]);
    bytes.push(VENDOR_ELEMENT);
    bytes.push((body.len() + 5) as u8);
    bytes.extend_from_slice(&ESPRESSIF);
    bytes.push(ESP_NOW_TYPE);
    bytes.push(VERSION);
    bytes.extend_from_slice(body);
    bytes
}

/// What came in, if it is ESP-NOW meant for `me` or for everyone: the sender and the body.
pub fn read<'a>(bytes: &'a [u8], me: &[u8; 6]) -> Option<([u8; 6], &'a [u8])> {
    let radiotap = usize::from(u16::from_le_bytes([*bytes.get(2)?, *bytes.get(3)?]));
    let frame = bytes.get(radiotap..)?;
    if *frame.first()? != ACTION {
        return None;
    }
    let to: [u8; 6] = frame.get(4..10)?.try_into().ok()?;
    if to != *me && to != BROADCAST {
        return None;
    }
    let from: [u8; 6] = frame.get(10..16)?.try_into().ok()?;
    let action = frame.get(HEADER..)?;
    if *action.first()? != VENDOR_ACTION || action.get(1..4)? != ESPRESSIF {
        return None;
    }
    let element = action.get(ACTION_START..)?;
    let [VENDOR_ELEMENT, size, oui @ .., ESP_NOW_TYPE, _version] = element.get(..7)? else {
        return None;
    };
    if *oui != ESPRESSIF {
        return None;
    }
    // A card may leave its checksum on the end, so the length comes from the element.
    let size = usize::from(*size).checked_sub(5)?;
    Some((from, element.get(7..7 + size)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ME: [u8; 6] = [1, 2, 3, 4, 5, 6];
    const BOARD: [u8; 6] = [0xAE, 0x27, 0x6E, 0xAA, 0xEF, 0x90];

    #[test]
    fn a_frame_reads_back() {
        let sent = frame(&ME, &BOARD, b"hello");
        assert_eq!(read(&sent, &ME), Some((BOARD, &b"hello"[..])));
    }

    #[test]
    fn a_checksum_left_on_the_end_stays_out_of_the_body() {
        let mut sent = frame(&BROADCAST, &BOARD, b"hello");
        sent.extend_from_slice(&[9, 9, 9, 9]);
        assert_eq!(read(&sent, &ME), Some((BOARD, &b"hello"[..])));
    }

    #[test]
    fn someone_elses_frames_and_other_wifi_are_passed_over() {
        let other = frame(&BOARD, &ME, b"hello");
        assert_eq!(read(&other, &ME), None);
        let mut beacon = frame(&BROADCAST, &BOARD, b"hello");
        beacon[8] = 0x80;
        assert_eq!(read(&beacon, &ME), None);
        let mut vendor = frame(&BROADCAST, &BOARD, b"hello");
        vendor[8 + HEADER + 1] = 0;
        assert_eq!(read(&vendor, &ME), None);
        assert_eq!(read(&[0, 0], &ME), None);
    }

    #[test]
    fn a_frame_cut_short_is_passed_over() {
        let mut cut = frame(&BROADCAST, &BOARD, b"hello");
        cut.truncate(cut.len() - 2);
        assert_eq!(read(&cut, &ME), None);
    }
}
