use super::*;
use alloc::boxed::Box;
use olc_core::{Chain, ChainError, CoinConfig, Consensus, Plugins};
use olc_pow::ProofOfWork;

fn chain() -> Chain {
    let mut config = CoinConfig::olc();
    config.initial_difficulty = 8;
    config.rules.coinbase_maturity = 1;
    config.rules.retarget_interval = 0;
    config.upgrade_from = 0;
    Chain::new(config, Plugins { consensus: Box::new(ProofOfWork), languages: olc_lang::all() })
}

fn mine(chain: &mut Chain, miner: &Address, txs: &[Tx]) {
    let time = chain.tip_header().time_ms + 1_000;
    let mut block = chain.block_template(miner, txs, time);
    assert!(ProofOfWork.try_seal(&mut block.header, 1_000_000));
    chain.add_block(block).unwrap();
}

fn people() -> [Wallet; 3] {
    [1u8, 2, 3].map(|seed| Wallet::from_secret([seed; 32]).unwrap())
}

fn two_of_three() -> Shared {
    let keys = people().iter().map(|person| person.public_keys().remove(0).1).collect();
    Shared::new(2, keys).unwrap()
}

/// Funds the shared wallet with one payment and gives its coin.
fn funded(chain: &mut Chain, shared: &Shared) -> (OutPoint, u64) {
    let payer = Wallet::from_secret([9; 32]).unwrap();
    for _ in 0..3 {
        mine(chain, &payer.address(), &[]);
    }
    let next = chain.height() + 1;
    let tx = payer.pay_lock_in(chain, GAMMA, &shared.lock(), 50_000, 0, &[]).unwrap();
    assert_eq!(chain.check_tx(&tx, next), Ok(0));
    mine(chain, &payer.address(), core::slice::from_ref(&tx));
    (OutPoint { txid: tx.id(), index: 0 }, 50_000)
}

#[test]
fn two_of_three_spend_with_any_two_people_and_not_with_one() {
    let mut chain = chain();
    let shared = two_of_three();
    let coin = funded(&mut chain, &shared);
    let [first, second, third] = people();
    let signing = chain.signing_id(chain.height() + 1);
    let mut spend = SharedSpend::new(&shared, &[coin], [7; 20], 20_000, 10, signing).unwrap();
    assert_eq!(spend.fill(&first), 1);
    assert_eq!(spend.missing(), 1);
    assert_eq!(spend.clone().finish(), Err(SharedError::Waiting(1)));

    // Passed on as text, then signed by the third person, not the second.
    let mut passed = SharedSpend::from_text(&spend.to_text()).unwrap();
    assert_eq!(passed, spend);
    assert_eq!(passed.fill(&second.clone()) + passed.fill(&third), 2);
    let tx = passed.finish().unwrap();
    assert_eq!(chain.check_tx(&tx, chain.height() + 1), Ok(10));
    let mut bad = tx.clone();
    bad.outputs[0].value -= 1;
    assert_eq!(chain.check_tx(&bad, chain.height() + 1), Err(ChainError::BadLock));
    assert_eq!(tx.outputs[1].lock, shared.lock());
    assert_eq!(tx.outputs[1].value, 50_000 - 20_000 - 10);

    // One person signing twice is still one signature.
    let mut alone = SharedSpend::new(&shared, &[coin], [7; 20], 20_000, 10, signing).unwrap();
    alone.fill(&first);
    alone.fill(&first);
    assert_eq!(alone.missing(), 1);
}

#[test]
fn a_wrong_signature_in_passed_text_is_dropped() {
    let mut chain = chain();
    let shared = two_of_three();
    let coin = funded(&mut chain, &shared);
    let [first, _, _] = people();
    let signing = chain.signing_id(chain.height() + 1);
    let mut spend = SharedSpend::new(&shared, &[coin], [7; 20], 20_000, 10, signing).unwrap();
    spend.fill(&first);
    spend.signatures[0][1] = Some(vec![5; 64]);
    let passed = SharedSpend::from_text(&spend.to_text()).unwrap();
    assert_eq!(passed.signed(), vec![0]);
}

#[test]
fn a_shared_wallet_reads_back_from_its_text_and_its_lock() {
    let shared = two_of_three();
    assert_eq!(Shared::from_text(&shared.to_text()), Ok(shared.clone()));
    assert_eq!(Shared::from_lock(&shared.lock()), Some(shared.clone()));
    let key = shared.keys[0].clone();
    assert_eq!(Shared::new(2, vec![key.clone(), key.clone()]), Err(SharedError::BadKey));
    assert_eq!(Shared::new(3, vec![key.clone(), shared.keys[1].clone()]), Err(SharedError::Shape));
    assert_eq!(Shared::new(1, vec![vec![4; 33]]), Err(SharedError::BadKey));
}

#[test]
fn twenty_keys_open_with_their_signatures() {
    let wallets: Vec<Wallet> = (1u8..=20).map(|seed| Wallet::from_secret([seed; 32]).unwrap()).collect();
    let keys = wallets.iter().map(|wallet| wallet.public_keys().remove(0).1).collect();
    let shared = Shared::new(15, keys).unwrap();
    let mut chain = chain();
    let coin = funded(&mut chain, &shared);
    let signing = chain.signing_id(chain.height() + 1);
    let mut spend = SharedSpend::new(&shared, &[coin], [7; 20], 1_000, 0, signing).unwrap();
    wallets.iter().skip(5).for_each(|wallet| {
        spend.fill(wallet);
    });
    let tx = spend.finish().unwrap();
    assert!(chain.check_tx(&tx, chain.height() + 1).is_ok());
    let mut bad = tx.clone();
    bad.outputs[0].value -= 1;
    assert_eq!(chain.check_tx(&bad, chain.height() + 1), Err(ChainError::BadLock));
}
