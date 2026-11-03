//! Which payments moved coins of an address, built while the wallet's history walks the blocks
//! this node keeps. A full node answers what an address holds; this answers how it got there.
use olc_core::{Address, Hash, OutPoint, Tx, ADDRESS_LANGUAGE};
use std::collections::{BTreeMap, VecDeque};

/// Moves kept over every address, the oldest dropped first: about four megabytes.
pub const MOST_MOVES: usize = 50_000;
/// Coins remembered, spent or not, so a spend can name the address and value of what it spent:
/// about ten megabytes, the oldest dropped first.
const MOST_COINS: usize = 100_000;
/// Moves one look gives back, newest first.
pub const SHOWN_MOVES: usize = 200;

/// One payment as it touched one address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AddressMove {
    pub height: u64,
    pub id: Hash,
    pub time_ms: u64,
    pub received: u64,
    pub spent: u64,
}

#[derive(Default)]
pub struct AddressIndex {
    moves: BTreeMap<Address, VecDeque<AddressMove>>,
    /// Each move's address in the order it came, to drop the oldest.
    order: VecDeque<Address>,
    coins: BTreeMap<OutPoint, (Address, u64)>,
    coin_order: VecDeque<OutPoint>,
    /// The first block read, and the block of the oldest move still kept.
    first: Option<u64>,
    oldest_kept: u64,
}

fn address_of(output: &olc_core::TxOut) -> Option<Address> {
    let plain = output.language == ADDRESS_LANGUAGE && !output.is_data();
    plain.then(|| <[u8; 20]>::try_from(output.lock.as_slice()).ok()).flatten()
}

impl AddressIndex {
    pub fn add(&mut self, height: u64, time_ms: u64, tx: &Tx) {
        self.first.get_or_insert(height);
        let id = tx.id();
        let mut touched: BTreeMap<Address, (u64, u64)> = BTreeMap::new();
        for input in &tx.inputs {
            if let Some((address, value)) = self.coins.get(&input.previous).copied() {
                touched.entry(address).or_default().1 += value;
            }
        }
        for (index, output) in tx.outputs.iter().enumerate() {
            let Some(address) = address_of(output) else {
                continue;
            };
            touched.entry(address).or_default().0 += output.value;
            let point = OutPoint { txid: id, index: index as u32 };
            self.coins.insert(point, (address, output.value));
            self.coin_order.push_back(point);
            if self.coin_order.len() > MOST_COINS {
                if let Some(oldest) = self.coin_order.pop_front() {
                    self.coins.remove(&oldest);
                }
            }
        }
        for (address, (received, spent)) in touched {
            self.moves.entry(address).or_default().push_back(AddressMove { height, id, time_ms, received, spent });
            self.order.push_back(address);
        }
        while self.order.len() > MOST_MOVES {
            let Some(oldest) = self.order.pop_front() else { break };
            if let Some(rows) = self.moves.get_mut(&oldest) {
                if let Some(row) = rows.pop_front() {
                    self.oldest_kept = self.oldest_kept.max(row.height + 1);
                }
                if rows.is_empty() {
                    self.moves.remove(&oldest);
                }
            }
        }
    }

    /// The newest moves of `address`, newest first.
    pub fn of(&self, address: &Address) -> Vec<AddressMove> {
        let rows = self.moves.get(address).into_iter().flat_map(|rows| rows.iter().rev());
        rows.take(SHOWN_MOVES).cloned().collect()
    }

    /// The address and value of a coin read here, spent or not.
    pub fn coin(&self, point: &OutPoint) -> Option<(Address, u64)> {
        self.coins.get(point).copied()
    }

    /// The first block whose moves are all here.
    pub fn from(&self) -> u64 {
        self.first.unwrap_or(0).max(self.oldest_kept)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use olc_core::{TxIn, TxOut, SEQUENCE_FINAL};

    fn output(to: Address, value: u64) -> TxOut {
        TxOut { value, language: ADDRESS_LANGUAGE, lock: to.to_vec() }
    }

    #[test]
    fn an_address_shows_what_came_in_and_what_left_over_the_blocks_read() {
        let mut index = AddressIndex::default();
        let mined = Tx { version: 1, inputs: Vec::new(), outputs: vec![output([1; 20], 50)], coinbase_height: Some(3), lock_time: 0 };
        index.add(3, 3_000, &mined);
        let spend = TxIn { previous: OutPoint { txid: mined.id(), index: 0 }, unlock: Vec::new(), sequence: SEQUENCE_FINAL };
        let paid = Tx { version: 2, inputs: vec![spend], outputs: vec![output([2; 20], 30), output([1; 20], 19)], coinbase_height: None, lock_time: 0 };
        index.add(4, 4_000, &paid);
        let moves = index.of(&[1; 20]);
        assert_eq!(moves.len(), 2);
        assert_eq!((moves[0].height, moves[0].received, moves[0].spent), (4, 19, 50));
        assert_eq!((moves[1].height, moves[1].received), (3, 50));
        assert_eq!(index.of(&[2; 20])[0].received, 30);
        assert_eq!(index.from(), 3);
    }
}
