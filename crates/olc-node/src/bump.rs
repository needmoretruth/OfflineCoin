//! Speeding up a payment that waits. One this wallet sent is signed again with a higher fee and
//! replaces the first (Bitcoin Core's bumpfee). One that pays this wallet gets a child that spends
//! what it pays with a fee high enough for both (child pays for parent). Either way the fee is
//! what puts it at the front of the next block, and at least what the pool's rules ask.
use crate::engine::Engine;
use crate::pool::INCREMENTAL_FEE;
use alloc::collections::BTreeSet;
use alloc::vec::Vec;
use olc_core::{ancestors, Address, Hash, OutPoint, Tx, TxIn, TxOut, Waiting, SEQUENCE_FINAL};
use olc_lang::ALPHA;
use olc_wallet::{Unsigned, WalletError};

/// A signature may come out a byte longer, and a value may take a byte more to write.
const SIZE_SLACK: u64 = 8;

/// What a speed up did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpedUp {
    /// The payment that now waits in its place, or the child that pulls it along.
    pub id: Hash,
    /// The fee the new payment pays.
    pub fee: u64,
    /// True when it replaced the payment, false when a child pulls it along.
    pub replaced: bool,
}

impl Engine {
    /// What a payment of `size` bytes pays to go ahead of every other waiting one when the next
    /// block has no room for them all, and nothing when it does.
    fn front_of_line(&self, rest: &[&Waiting], size: u64) -> u64 {
        let room = self.chain.room_for_payments(self.note.len());
        let waiting: u64 = rest.iter().map(|entry| entry.size).sum();
        if waiting.saturating_add(size) <= room {
            return 0;
        }
        let best = rest.iter().map(|entry| entry.fee.div_ceil(entry.size.max(1))).max().unwrap_or(0);
        best.saturating_add(1).saturating_mul(size)
    }

    /// The address of this wallet a coin pays, when it does.
    fn owner_of(&self, output: &TxOut) -> Option<Address> {
        let address: Address = output.lock.as_slice().try_into().ok()?;
        (output.language == ALPHA && self.wallet.owns(&address)).then_some(address)
    }

    /// What speeding up `id` would cost: the fee it pays now, and the fee it would pay. Nothing
    /// is signed or sent, so a screen can show both before the person agrees.
    pub fn speed_up_fees(&self, id: &Hash) -> Result<(u64, u64), WalletError> {
        let at = self.pool.iter().position(|tx| tx.id() == *id).ok_or(WalletError::NoSuchKey)?;
        let now = self.chain.waiting(&self.pool).get(at).map_or(0, |entry| entry.fee);
        let (_, fee, _) = self.plan_speed_up(id, false)?;
        Ok((now, fee))
    }

    /// Speeds up the waiting payment `id`, sent by this wallet or paying it.
    pub fn speed_up(&mut self, id: &Hash) -> Result<SpedUp, WalletError> {
        let (tx, fee, replaced) = self.plan_speed_up(id, true)?;
        let id = self.send_payment(tx)?;
        Ok(SpedUp { id, fee, replaced })
    }

    /// The payment that would take the place of `id`, or the child that would pull it along,
    /// with its fee. Signed only when `sign` is set; the fee does not depend on it.
    fn plan_speed_up(&self, id: &Hash, sign: bool) -> Result<(Tx, u64, bool), WalletError> {
        let at = self.pool.iter().position(|tx| tx.id() == *id).ok_or(WalletError::NoSuchKey)?;
        let tx = self.pool[at].clone();
        let owners: Option<Vec<Address>> = tx
            .inputs
            .iter()
            .map(|input| self.chain.coin_at(&input.previous).and_then(|coin| self.owner_of(&coin.output)))
            .collect();
        match owners {
            Some(owners) => self.replace(at, tx, owners, sign).map(|(tx, fee)| (tx, fee, true)),
            None => self.pull_along(at, tx).map(|(tx, fee)| (tx, fee, false)),
        }
    }

    /// Signs `tx` again from the same coins with a higher fee, taken from its change, and from
    /// another coin of this wallet when the change is too small.
    fn replace(&self, at: usize, mut tx: Tx, mut owners: Vec<Address>, sign: bool) -> Result<(Tx, u64), WalletError> {
        let waiting = self.chain.waiting(&self.pool);
        let mut replaced = BTreeSet::from([at]);
        for (place, entry) in waiting.iter().enumerate() {
            if entry.parents.iter().any(|parent| replaced.contains(parent)) {
                replaced.insert(place);
            }
        }
        let pushed_out = replaced.iter().map(|place| waiting[*place].fee).fold(0, u64::saturating_add);
        let old = &waiting[at];
        let rest: Vec<&Waiting> = (0..waiting.len()).filter(|place| !replaced.contains(place)).map(|place| &waiting[place]).collect();
        let next = self.chain.height() + 1;
        let coin = self.chain.signing_id(next).filter(|_| tx.version > 1);
        let reserved = self.reserved();
        let mut extra_coins = self.wallet_coins(&reserved).into_iter();
        loop {
            let size = tx.encode().len() as u64 + SIZE_SLACK;
            let rules = pushed_out.saturating_add(INCREMENTAL_FEE.saturating_mul(size));
            let beats = (u128::from(old.fee) * u128::from(size) / u128::from(old.size.max(1))) as u64 + 1;
            let fee = rules.max(beats).max(self.front_of_line(&rest, size));
            let spent: u64 = tx.inputs.iter().filter_map(|input| self.chain.coin_at(&input.previous)).map(|coin| coin.output.value).sum();
            let change_at = tx.outputs.iter().rposition(|output| self.owner_of(output).is_some() && !output.is_data());
            let paid: u64 = tx.outputs.iter().enumerate().filter(|(place, _)| Some(*place) != change_at).map(|(_, output)| output.value).sum();
            if let Some(change) = spent.checked_sub(paid).and_then(|left| left.checked_sub(fee)) {
                match change_at {
                    Some(place) if change > 0 => tx.outputs[place].value = change,
                    Some(place) => {
                        tx.outputs.remove(place);
                    }
                    None if change > 0 => tx.outputs.push(TxOut { value: change, language: ALPHA, lock: self.wallet.address().to_vec() }),
                    None => {}
                }
                if !sign {
                    return Ok((tx, fee));
                }
                return Ok((self.wallet.sign(&Unsigned { tx, owners, coin })?, fee));
            }
            let (point, owner) = extra_coins.next().ok_or(WalletError::NotEnoughCoins)?;
            tx.inputs.push(TxIn { previous: point, unlock: Vec::new(), sequence: SEQUENCE_FINAL });
            owners.push(owner);
        }
    }

    /// Mature coins of this wallet that nothing waiting spends yet.
    fn wallet_coins(&self, reserved: &[OutPoint]) -> Vec<(OutPoint, Address)> {
        let next = self.chain.height() + 1;
        let mut coins = Vec::new();
        for (address, _) in self.wallet.keys() {
            for (point, entry) in self.chain.unspent_of(&address) {
                if self.chain.is_mature(&entry, next) && !reserved.contains(&point) {
                    coins.push((point, address));
                }
            }
        }
        coins
    }

    /// Spends what the waiting payment at `at` pays this wallet back to it, with a fee that
    /// carries the payment and every waiting one it needs.
    fn pull_along(&self, at: usize, parent: Tx) -> Result<(Tx, u64), WalletError> {
        let txid = parent.id();
        let reserved = self.reserved();
        let ours: Vec<(OutPoint, Address, u64)> = parent
            .outputs
            .iter()
            .enumerate()
            .filter_map(|(index, output)| Some((OutPoint { txid, index: index as u32 }, self.owner_of(output)?, output.value)))
            .filter(|(point, _, _)| !reserved.contains(point))
            .collect();
        if ours.is_empty() {
            return Err(WalletError::NoSuchKey);
        }
        let waiting = self.chain.waiting(&self.pool);
        let needed = ancestors(&waiting, at);
        let (package_fee, package_size) = needed.iter().fold((0u64, 0u64), |(fee, size), place| {
            (fee.saturating_add(waiting[*place].fee), size.saturating_add(waiting[*place].size))
        });
        let rest: Vec<&Waiting> = (0..waiting.len()).filter(|place| !needed.contains(place)).map(|place| &waiting[place]).collect();
        let value: u64 = ours.iter().map(|(_, _, value)| *value).sum();
        let coin = self.chain.signing_id(self.chain.height() + 1).filter(|_| parent.version > 1);
        let child = |fee: u64| -> Result<Tx, WalletError> {
            let left = value.checked_sub(fee).filter(|left| *left > 0).ok_or(WalletError::NotEnoughCoins)?;
            let inputs = ours.iter().map(|(point, _, _)| TxIn { previous: *point, unlock: Vec::new(), sequence: SEQUENCE_FINAL });
            let tx = Tx {
                version: olc_core::types::version_for(coin.as_ref()),
                inputs: inputs.collect(),
                outputs: alloc::vec![TxOut { value: left, language: ALPHA, lock: self.wallet.address().to_vec() }],
                coinbase_height: None,
                lock_time: 0,
            };
            let owners = ours.iter().map(|(_, owner, _)| *owner).collect();
            self.wallet.sign(&Unsigned { tx, owners, coin })
        };
        let size = child(0)?.encode().len() as u64 + SIZE_SLACK;
        let together = package_size + size;
        let front = self.front_of_line(&rest, together);
        let fee = front.saturating_sub(package_fee).max(INCREMENTAL_FEE.saturating_mul(size));
        Ok((child(fee)?, fee))
    }
}
