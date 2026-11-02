//! Message framing for byte streams such as TCP: four bytes of length, then the message.
use alloc::vec::Vec;

const HEADER: usize = 4;
const MAX_MESSAGE: usize = 8 * 1024 * 1024;
/// The longest message a board takes, unless its coin has larger blocks. Anyone in reach can open
/// a connection and claim a length, so this much per connection must fit its memory.
pub const MOST_ON_A_BOARD: usize = 64 * 1024;

pub fn encode(message: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(HEADER + message.len());
    frame.extend_from_slice(&(message.len() as u32).to_le_bytes());
    frame.extend_from_slice(message);
    frame
}

pub struct Decoder {
    buffer: Vec<u8>,
    most: usize,
}

impl Default for Decoder {
    fn default() -> Self {
        Self::at_most(MAX_MESSAGE)
    }
}

impl Decoder {
    /// A decoder that breaks at any message longer than `most` bytes.
    pub fn at_most(most: usize) -> Self {
        Self { buffer: Vec::new(), most }
    }

    pub fn push(&mut self, bytes: &[u8]) {
        self.buffer.extend_from_slice(bytes);
    }

    /// True when the stream cannot be OfflineCoin: drop the connection.
    pub fn is_broken(&self) -> bool {
        self.size().is_some_and(|size| size > self.most)
    }

    pub fn next_message(&mut self) -> Option<Vec<u8>> {
        let size = self.size().filter(|size| *size <= self.most)?;
        if self.buffer.len() < HEADER + size {
            return None;
        }
        let message = self.buffer[HEADER..HEADER + size].to_vec();
        self.buffer.drain(..HEADER + size);
        Some(message)
    }

    fn size(&self) -> Option<usize> {
        let head = self.buffer.get(..HEADER)?;
        Some(u32::from_le_bytes([head[0], head[1], head[2], head[3]]) as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_survive_any_chunking() {
        let mut stream = encode(b"first");
        stream.extend(encode(b""));
        stream.extend(encode(&[9; 300]));
        let mut decoder = Decoder::default();
        let mut found = Vec::new();
        for byte in stream {
            decoder.push(&[byte]);
            found.extend(decoder.next_message());
        }
        assert_eq!(found, [b"first".to_vec(), Vec::new(), [9; 300].to_vec()]);
    }

    #[test]
    fn a_board_breaks_at_its_own_limit() {
        let mut decoder = Decoder::at_most(MOST_ON_A_BOARD);
        decoder.push(&((MOST_ON_A_BOARD + 1) as u32).to_le_bytes());
        assert!(decoder.is_broken());
        assert_eq!(decoder.next_message(), None);
    }

    #[test]
    fn an_impossible_length_breaks_the_stream() {
        let mut decoder = Decoder::default();
        decoder.push(&[0xFF; 4]);
        assert!(decoder.is_broken());
    }
}
