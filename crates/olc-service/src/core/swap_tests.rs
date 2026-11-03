use super::*;
use crate::core::swap_text::{coin_id, find_coin, OfferText, MOST_ADDRESSES};
use olc_core::coin::{CoinConfig, COIN, NEVER};
use olc_core::Plugins;
use olc_wallet::Wallet;

/// A coin as Satoshi left it, whose payments are all version 1.
fn old_coin() -> CoinConfig {
    let mut coin = CoinConfig::olc();
    coin.initial_difficulty = 8;
    coin.rules.coinbase_maturity = 1;
    coin.rules.retarget_interval = 0;
    coin.upgrade_from = NEVER;
    coin
}

/// A coin made since, whose payments are version 2 from its genesis.
fn new_coin() -> CoinConfig {
    let mut coin = old_coin();
    coin.message = "another coin".into();
    coin.ticker = "TWO".into();
    coin.upgrade_from = 0;
    coin
}

fn engine(coin: CoinConfig, seed: u8) -> Engine {
    let plugins = Plugins { consensus: Box::new(olc_pow::ProofOfWork), languages: olc_lang::all() };
    Engine::new(coin, plugins, Wallet::from_secret([seed; 32]).unwrap())
}

/// Two nodes of one coin: each person's.
struct Coin {
    maker: Engine,
    taker: Engine,
}

impl Coin {
    fn new(coin: CoinConfig) -> Self {
        Self { maker: engine(coin.clone(), 1), taker: engine(coin, 2) }
    }

    /// One block from `by`'s node with both pools in it, which the other node takes too.
    fn mine(&mut self, by: Role) {
        let (miner, other) = match by {
            Role::Maker => (&mut self.maker, &mut self.taker),
            Role::Taker => (&mut self.taker, &mut self.maker),
        };
        for tx in other.pool().to_vec() {
            miner.send_signed(tx);
        }
        let now = miner.chain().tip_header().time_ms + miner.chain().rules().block_time_ms;
        while !miner.mine(100_000, now) {}
        while miner.take_outgoing().is_some() {}
        let block = miner.chain().blocks_from(miner.chain().height(), 1).remove(0);
        other.chain_mut().add_block(block).unwrap();
    }

    fn side(&self, units: u64) -> Side {
        Side { coin: self.maker.chain().config().clone(), units }
    }
}

/// The maker has coins on `old` and the taker on `new`, and the taker answered an offer.
fn agreed(wait_minutes: u32) -> (Coin, Coin, Swap, Swap) {
    let (mut old, mut new) = (Coin::new(old_coin()), Coin::new(new_coin()));
    for _ in 0..4 {
        old.mine(Role::Maker);
        new.mine(Role::Taker);
    }
    let (give, take) = (old.side(5 * COIN), new.side(7 * COIN));
    let mut made = make_offer(&old.maker, (give, take), wait_minutes, vec!["127.0.0.1:7801".into()]).unwrap();
    let read = OfferText::from_text(&made.text()).unwrap();
    let offer = read.with_coins(old_coin(), new_coin()).unwrap();
    assert_eq!(offer, made.offer, "an offer goes over as text");
    let taken = take_offer(offer, &new.taker, vec!["127.0.0.1:7901".into()]);
    made.accept = Some(Accept::from_text(&taken.text()).unwrap());
    assert_eq!(made.locks(), taken.locks(), "both sides write the same two locks");
    (old, new, made, taken)
}

#[test]
fn a_swap_moves_each_sides_coins_to_the_other() {
    let (mut old, mut new, mut made, mut taken) = agreed(10);
    let (maker_had, taker_had) = (old.taker.chain().balance(&old.taker.wallet().address()), 0);
    let mut heard = Vec::new();
    for _ in 0..12 {
        heard.extend(made.step(&mut old.maker, &mut new.maker).map(|happened| (Role::Maker, happened)));
        heard.extend(taken.step(&mut new.taker, &mut old.taker).map(|happened| (Role::Taker, happened)));
        old.mine(Role::Maker);
        new.mine(Role::Taker);
    }
    assert_eq!(made.outcome, Some(Outcome::Swapped), "{heard:?}");
    assert_eq!(taken.outcome, Some(Outcome::Swapped), "{heard:?}");
    assert_eq!(maker_had, taker_had, "the taker had nothing on the maker's coin");
    let fee = old.maker.rules().min_fee;
    assert_eq!(old.taker.chain().balance(&old.taker.wallet().address()), 5 * COIN - fee);
    let fee = new.maker.rules().min_fee;
    assert_eq!(new.maker.chain().balance(&new.maker.wallet().address()), 7 * COIN - fee);
    let order: Vec<_> = heard.iter().map(|(role, happened)| (*role, *happened)).collect();
    assert_eq!(&order[..3], &[(Role::Maker, Happened::Locked), (Role::Taker, Happened::Locked), (Role::Maker, Happened::Claimed)]);
}

#[test]
fn a_maker_left_waiting_takes_its_coins_back_at_the_height_and_not_before() {
    let (mut old, mut new, mut made, _) = agreed(10);
    let maker = old.maker.wallet().address();
    let before = old.maker.chain().balance(&maker);
    let refund_at = made.offer.refund_at;
    assert_eq!(made.step(&mut old.maker, &mut new.maker), Some(Happened::Locked));
    old.mine(Role::Taker);
    while old.maker.chain().height() + 1 < refund_at {
        assert_eq!(made.step(&mut old.maker, &mut new.maker), None, "the taker never locked");
        old.mine(Role::Taker);
    }
    assert_eq!(made.stage(), Stage::Locked);
    assert_eq!(made.step(&mut old.maker, &mut new.maker), Some(Happened::Refunded));
    old.mine(Role::Taker);
    made.step(&mut old.maker, &mut new.maker);
    assert_eq!(made.outcome, Some(Outcome::Refunded));
    let fee = old.maker.rules().min_fee;
    assert_eq!(old.maker.chain().balance(&maker), before - 2 * fee);
}

#[test]
fn a_taker_does_not_lock_once_the_makers_way_back_is_near() {
    let (mut old, mut new, mut made, mut taken) = agreed(10);
    made.step(&mut old.maker, &mut new.maker);
    while old.taker.chain().height() + blocks_in(&old.taker, 10) <= made.offer.refund_at {
        old.mine(Role::Maker);
    }
    assert_eq!(taken.step(&mut new.taker, &mut old.taker), Some(Happened::TooLate));
    assert_eq!(taken.my_lock, None);
}

#[test]
fn the_secret_is_read_back_from_the_unlock_that_used_it() {
    let secret = [9u8; 32];
    let hash20: [u8; 20] = hash(&secret)[..20].try_into().unwrap();
    let words = format!("0x{} 0x{} 1", hex(&[1; 64]), hex(&secret));
    let unlock = lock_from_text(&words).unwrap().0;
    assert_eq!(secret_in(&unlock, &hash20), Some(secret.to_vec()));
    assert_eq!(secret_in(&unlock, &[0; 20]), None);
    assert_eq!(OfferText::from_text("olcswap1.zz"), None);
    assert_eq!(Accept::from_text("hello"), None);
}

/// A coin with rules the binary form of a coin leaves out, and an offer of OLC for it.
fn offer_for_another_coin() -> (CoinConfig, Offer) {
    let mut other = new_coin();
    other.rules.floor_from_height = 3;
    other.message_in_block = true;
    let side = |coin: &CoinConfig| Side { coin: coin.clone(), units: 5 * COIN };
    let at = vec!["192.168.0.20:7800".to_string(), "127.0.0.1:7800".to_string()];
    let (give, take) = (side(&CoinConfig::olc()), side(&other));
    (other, Offer { give, take, hash: [1; 20], maker: [2; 20], refund_at: 9, wait_minutes: 10, at })
}

#[test]
fn an_offer_text_names_each_coin_by_id_and_carries_no_coin_file() {
    let (other, offer) = offer_for_another_coin();
    let text = offer.to_text();
    let file = olc_store::coin_file::to_json(&other).unwrap();
    assert!(!text.contains(&hex(file.as_bytes())[..40]), "the coin file stays out of the text");
    assert!(text.len() < 400, "the text is {} characters long", text.len());
    let read = OfferText::from_text(&text).unwrap();
    assert_eq!((read.give.id, read.take.id), (coin_id(&CoinConfig::olc()), coin_id(&other)));
    assert_eq!(read.id(), offer.id(), "both sides name the swap alike");
    assert_eq!(read.clone().with_coins(CoinConfig::olc(), other.clone()), Some(offer.clone()));
    // A fork keeps the genesis block and the ticker, and differs by its rules: still another coin.
    let mut fork = other.clone();
    fork.rules.min_fee += 1;
    assert_eq!(fork.genesis().id(), other.genesis().id());
    assert_eq!(read.with_coins(CoinConfig::olc(), fork), None);
    // The wallet keeps the whole coin beside the swap.
    let kept: Offer = serde_json::from_str(&serde_json::to_string(&offer).unwrap()).unwrap();
    assert_eq!(kept, offer);
}

#[test]
fn a_coin_fetched_from_the_maker_counts_only_when_it_hashes_to_the_named_id() {
    let (other, offer) = offer_for_another_coin();
    let read = OfferText::from_text(&offer.to_text()).unwrap();
    let mut fork = other.clone();
    fork.rules.min_fee += 1;
    let at = ["10.0.0.1:7800".to_string(), "10.0.0.2:7800".to_string()];
    let liar = |_: &str| Some(fork.clone());
    assert_eq!(find_coin(&read.take, [], &at, liar), None, "another coin under the same name is refused");
    let second = |address: &str| (address == at[1]).then(|| other.clone()).or_else(|| Some(fork.clone()));
    assert_eq!(find_coin(&read.take, [], &at, second), Some(other.clone()));
    let never = |_: &str| -> Option<CoinConfig> { panic!("a coin at hand needs no asking") };
    assert_eq!(find_coin(&read.take, [CoinConfig::olc(), other.clone()], &at, never), Some(other));
    let mut crowded = offer;
    crowded.at = (0..=MOST_ADDRESSES).map(|n| format!("10.0.0.{n}:7800")).collect();
    assert_eq!(OfferText::from_text(&crowded.to_text()), None, "a text with too many addresses is not this app's");
}

/// The taker's node asks the maker's node, on a real socket, for the coin an offer names.
#[test]
fn the_taker_fetches_the_makers_coin_from_its_node() {
    use crate::core::{Host, Plan};
    use crate::discovery::ask;
    use std::time::Duration;
    let (other, offer) = offer_for_another_coin();
    let folder = std::env::temp_dir().join(format!("olc-swap-coin-{}", std::process::id()));
    let plan = Plan {
        coin: other.clone(),
        folder: folder.join("chain"),
        wallet_folder: folder.join("wallet"),
        bind_host: "127.0.0.1".into(),
        port: 7360,
        port_tries: 20,
        peers: Vec::new(),
        cap: 0,
        mining: false,
        card: olc_node::Card::default(),
        storage: None,
        shares_wallet: false,
    };
    let host = Host::start(plan, None).unwrap();
    let mut port = None;
    for _ in 0..100 {
        port = host.view().port.filter(|_| host.view().coin.is_some());
        if port.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // The first address is where nothing listens, as a maker's address on another network.
    let at = vec!["127.0.0.1:1".to_string(), format!("127.0.0.1:{}", port.unwrap())];
    let fetch = |address: &str| ask(address, Duration::from_secs(2)).map(|hello| hello.coin);
    let read = OfferText::from_text(&offer.to_text()).unwrap();
    assert_eq!(find_coin(&read.take, [], &at, fetch), Some(other.clone()));
    let mut fork = other;
    fork.rules.min_fee += 1;
    let named_fork = Offer { take: Side { coin: fork, units: 1 }, ..offer }.as_text().take;
    assert_eq!(find_coin(&named_fork, [], &at, fetch), None, "that node runs another coin than the one named");
    drop(host);
    let _ = std::fs::remove_dir_all(folder);
}
