//! Splits messages for small-frame transports such as ESP-NOW, and joins them again.
use alloc::vec;
use alloc::vec::Vec;

/// network mark (4 bytes), message id (2 bytes), fragment index, fragment count. The mark leads
/// every frame so a receiver can drop one without keeping anything.
pub const HEADER: usize = 8;
const MARK: usize = 4;
const MAX_PARTIALS: usize = 8;

/// `message` starts with its mark, which moves into each frame's header.
pub fn split(id: u16, message: &[u8], frame_size: usize) -> Vec<Vec<u8>> {
    let payload = frame_size.saturating_sub(HEADER).max(1);
    let (mark, body) = message.split_at(MARK.min(message.len()));
    let pieces: Vec<&[u8]> = body.chunks(payload).collect();
    let count = pieces.len().min(255) as u8;
    pieces
        .iter()
        .take(255)
        .enumerate()
        .map(|(index, piece)| {
            let mut frame = Vec::with_capacity(HEADER + piece.len());
            frame.extend_from_slice(mark);
            frame.extend_from_slice(&id.to_le_bytes());
            frame.push(index as u8);
            frame.push(count);
            frame.extend_from_slice(piece);
            frame
        })
        .collect()
}

struct Partial {
    source: u32,
    id: u16,
    pieces: Vec<Option<Vec<u8>>>,
}

#[derive(Default)]
pub struct Reassembler {
    partials: Vec<Partial>,
}

impl Reassembler {
    /// Returns the whole message once its last fragment has arrived.
    pub fn push(&mut self, source: u32, frame: &[u8]) -> Option<Vec<u8>> {
        if frame.len() < HEADER {
            return None;
        }
        let id = u16::from_le_bytes([frame[MARK], frame[MARK + 1]]);
        let (index, count) = (usize::from(frame[MARK + 2]), usize::from(frame[MARK + 3]));
        if index >= count {
            return None;
        }
        let found = self
            .partials
            .iter()
            .position(|partial| partial.source == source && partial.id == id);
        let slot = match found {
            Some(slot) => slot,
            None => {
                if self.partials.len() >= MAX_PARTIALS {
                    self.partials.remove(0);
                }
                self.partials.push(Partial {
                    source,
                    id,
                    pieces: vec![None; count],
                });
                self.partials.len() - 1
            }
        };
        let partial = &mut self.partials[slot];
        if partial.pieces.len() != count {
            return None;
        }
        partial.pieces[index] = Some(frame[HEADER..].to_vec());
        if partial.pieces.iter().any(Option::is_none) {
            return None;
        }
        let done = self.partials.remove(slot);
        let mut message = frame[..MARK].to_vec();
        message.extend(done.pieces.into_iter().flatten().flatten());
        Some(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn out_of_order_fragments_join() {
        let message: Vec<u8> = (0..=255u8).cycle().take(1_000).collect();
        let mut frames = split(7, &message, 250);
        assert!(frames.iter().all(|frame| frame.len() <= 250));
        frames.reverse();
        let mut joiner = Reassembler::default();
        let mut result = None;
        for frame in &frames {
            result = joiner.push(1, frame).or(result);
        }
        assert_eq!(result, Some(message));
    }

    #[test]
    fn a_missing_fragment_yields_nothing() {
        let frames = split(1, &[0u8; 600], 250);
        let mut joiner = Reassembler::default();
        assert!(frames[1..]
            .iter()
            .all(|frame| joiner.push(1, frame).is_none()));
    }
}
