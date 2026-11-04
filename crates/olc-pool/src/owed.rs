//! Payments the pool owes and has not sent. The node's wallet may be locked when a block's coins
//! ripen, and the app may close or the pool stop before it opens, so whoever runs the pool keeps
//! them outside it and pays them here once the wallet signs again.
use olc_core::{Address, Hash};
use olc_node::Engine;
use olc_wallet::WalletError;

/// One block's payment to the workers, as the pool worked it out when the block was found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Owed {
    /// The block whose coins pay it: a block that left the chain pays nothing.
    pub height: u64,
    pub block: Hash,
    pub payments: Vec<(Address, u64)>,
    pub fee: u64,
}

/// Pays what is owed, each payment once its block's coins can be spent, and keeps what still
/// waits. A block that left the chain is dropped, since its coins are gone. True when the wallet
/// is locked and a ripe payment waits for it to open.
pub fn pay_owed(engine: &mut Engine, owed: &mut Vec<Owed>) -> bool {
    let mut held = false;
    owed.retain(|debt| {
        let chain = engine.chain();
        if chain.rules().matures_at(debt.height) > chain.height() {
            return true;
        }
        match chain.id_at(debt.height) {
            Some(id) if id == debt.block => {}
            Some(_) => return false,
            None => return true,
        }
        match engine.pay_many(&debt.payments, debt.fee) {
            Ok(_) => false,
            Err(error) => {
                held |= error == WalletError::Locked;
                true
            }
        }
    });
    held
}
