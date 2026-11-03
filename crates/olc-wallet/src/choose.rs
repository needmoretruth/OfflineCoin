//! A payment whose fee follows a rate per byte, from coins a person chose, as Bitcoin Core's
//! custom fee and coin control do.
use crate::{prepare_from, Order, Wallet, WalletError};
use alloc::vec;
use alloc::vec::Vec;
use olc_core::{Address, Coins, OutPoint, Tx, TxIn, TxOut, SEQUENCE_FINAL};
use olc_lang::{alpha, ALPHA};

/// A fee that grows with the size it pays for settles within two rounds; a few more cover a
/// coin added on the way.
const ROUNDS: usize = 6;

/// The fee of a payment `size` bytes long at `rate` units a byte, never under the coin's lowest.
pub fn fee_for(rate: u64, size: u64, min_fee: u64) -> u64 {
    rate.saturating_mul(size).max(min_fee)
}

/// About how many bytes a signed payment of this wallet's kind takes, with `inputs` coins in and
/// `outputs` out. The screen uses it to show a fee before anything is built.
pub fn estimated_size(inputs: usize, outputs: usize) -> u64 {
    // The longest unlock and a large value, so the guess errs high.
    let input = TxIn { previous: OutPoint { txid: [0xAB; 32], index: 0 }, unlock: vec![0xAB; alpha::UNLOCK_LEN], sequence: SEQUENCE_FINAL };
    let output = TxOut { value: 5_000_000_000, language: ALPHA, lock: vec![0xAB; 20] };
    let tx = Tx { version: 2, inputs: vec![input; inputs.max(1)], outputs: vec![output; outputs.max(1)], coinbase_height: None, lock_time: 0 };
    tx.encode().len() as u64
}

impl Wallet {
    /// Pays every `(address, amount)` in one payment at `rate` units a byte, never under
    /// `min_fee`. Spends only the coins in `only` when it names any. Gives the signed payment
    /// and its fee.
    pub fn pay_at_rate(
        &self,
        chain: &dyn Coins,
        payments: &[(Address, u64)],
        (rate, min_fee): (u64, u64),
        reserved: &[OutPoint],
        only: &[OutPoint],
    ) -> Result<(Tx, u64), WalletError> {
        if payments.is_empty() || payments.iter().any(|(_, amount)| *amount == 0) {
            return Err(WalletError::ZeroAmount);
        }
        let owners: Vec<Address> = self.keys().map(|(address, _)| address).collect();
        let mut fee = min_fee;
        for _ in 0..ROUNDS {
            let order = Order { payments, data: &[], locked: None, fee, change: self.address() };
            let tx = self.sign(&prepare_from(chain, &owners, &order, reserved, only)?)?;
            let wanted = fee_for(rate, tx.encode().len() as u64, min_fee);
            if wanted <= fee {
                return Ok((tx, fee));
            }
            fee = wanted;
        }
        Err(WalletError::NotEnoughCoins)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::boxed::Box;
    use olc_core::{Chain, CoinConfig, Plugins};
    use olc_pow::ProofOfWork;

    /// A wallet that mined four blocks, two of them old enough to spend.
    fn funded() -> (Wallet, Chain) {
        let wallet = Wallet::from_secret([7; 32]).unwrap();
        let mut config = CoinConfig::olc();
        config.initial_difficulty = 8;
        config.rules.coinbase_maturity = 2;
        config.rules.retarget_interval = 0;
        let mut chain = Chain::new(config, Plugins { consensus: Box::new(ProofOfWork), languages: olc_lang::all() });
        for _ in 0..4 {
            let time = chain.tip_header().time_ms + 1_000;
            let mut block = chain.block_template(&wallet.address(), &[], time);
            assert!(olc_core::Consensus::try_seal(&ProofOfWork, &mut block.header, 1_000_000));
            chain.add_block(block).unwrap();
        }
        (wallet, chain)
    }

    #[test]
    fn the_fee_follows_the_rate_and_the_size() {
        let (wallet, chain) = funded();
        let to = [9; 20];
        let (tx, fee) = wallet.pay_at_rate(&chain, &[(to, 10_000)], (10, 1), &[], &[]).unwrap();
        assert_eq!(fee, 10 * tx.encode().len() as u64);
        let (_, lowest) = wallet.pay_at_rate(&chain, &[(to, 10_000)], (0, 7), &[], &[]).unwrap();
        assert_eq!(lowest, 7);
        // The guess is never under the real size, so the fee a sheet shows is never short.
        let (size, real) = (estimated_size(tx.inputs.len(), tx.outputs.len()), tx.encode().len() as u64);
        assert!(size >= real && size - real <= 16, "{size} against {real}");
    }

    #[test]
    fn chosen_coins_are_the_only_ones_spent() {
        let (wallet, chain) = funded();
        let coins = chain.unspent_of(&wallet.address());
        // Left to itself the wallet takes the first coin; the person picked the last that is ready.
        let mut ripe = coins.iter().filter(|(_, coin)| chain.is_mature(coin, chain.height() + 1));
        let big = ripe.next_back().unwrap().0;
        assert_ne!(big, coins[0].0);
        let (tx, _) = wallet.pay_at_rate(&chain, &[([9; 20], 10_000), ([8; 20], 20_000)], (0, 1), &[], &[big]).unwrap();
        assert_eq!(tx.inputs.len(), 1);
        assert_eq!(tx.inputs[0].previous, big);
        assert_eq!(tx.outputs.len(), 3, "two people and the change");
    }
}
