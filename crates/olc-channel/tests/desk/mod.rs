//! A desk of channel nodes on one real chain: each has its own folder, wallet and files, the
//! messages between them are passed as the links would, and blocks are mined with what they
//! published. A node can be cut off, and opened again from its folder.
#![allow(dead_code)]
use olc_channel::{ChannelNode, Ground, NodeId, Out};
use olc_core::types::merkle_root;
use olc_core::{Address, Chain, CoinConfig, Consensus, Hash, OutPoint, Plugins, Tx, TxOut};
use olc_pow::ProofOfWork;
use olc_wallet::Wallet;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Every file the writer below made, in every test of this binary.
static WRITTEN: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// The tests' writer: a new file, flushed, as the host's writer makes it, and noted.
fn write_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    WRITTEN.lock().map_err(|_| std::io::Error::other("a test failed while writing"))?.push(path.to_path_buf());
    Ok(())
}

/// The files in `folder` that the node did not make through the writer it was given.
pub fn written_around(folder: &Path) -> Vec<PathBuf> {
    let written = WRITTEN.lock().unwrap().clone();
    let files = std::fs::read_dir(folder).unwrap().flatten().map(|entry| entry.path());
    files.filter(|file| !written.contains(&file.with_extension("new"))).collect()
}

pub const FEE: u64 = 1_000;
pub const DELAY: u16 = 6;

pub struct World {
    pub chain: Chain,
    pub pool: Vec<Tx>,
    pub now_ms: u64,
    miner: Wallet,
}

pub struct Party {
    pub name: &'static str,
    pub node: ChannelNode,
    pub wallet: Wallet,
    pub folder: PathBuf,
    /// Hears and says nothing while true.
    pub cut: bool,
}

struct Desk<'a> {
    world: &'a mut World,
    wallet: &'a Wallet,
}

impl Ground for Desk<'_> {
    fn height(&self) -> u64 {
        self.world.chain.height()
    }

    fn block_at(&self, height: u64) -> Option<(Hash, Vec<Tx>)> {
        let id = self.world.chain.id_at(height)?;
        Some((id, self.world.chain.block(&id)?.txs))
    }

    fn coin(&self, point: &OutPoint) -> Option<(TxOut, u64)> {
        self.world.chain.coin_at(point).map(|coin| (coin.output.clone(), coin.height))
    }

    fn coin_id(&self) -> Option<Hash> {
        self.world.chain.signing_id(self.world.chain.height() + 1)
    }

    fn genesis(&self) -> Hash {
        self.world.chain.genesis_id()
    }

    fn fee(&self) -> u64 {
        FEE
    }

    fn pay_to(&self) -> Address {
        self.wallet.address()
    }

    fn fund(&mut self, lock: &[u8], amount: u64) -> Result<Tx, String> {
        let reserved: Vec<OutPoint> = self.world.pool.iter().flat_map(|tx| tx.inputs.iter().map(|input| input.previous)).collect();
        self.wallet.pay_lock_in(&self.world.chain, olc_lang::GAMMA, lock, amount, FEE, &reserved).map_err(|error| format!("{error:?}"))
    }

    fn publish(&mut self, tx: &Tx) -> Result<(), String> {
        if self.world.pool.iter().any(|known| known.id() == tx.id()) {
            return Ok(());
        }
        let height = self.world.chain.height() + 1;
        self.world.chain.check_tx(tx, height).map_err(|error| format!("{error:?}"))?;
        let spent: Vec<OutPoint> = self.world.pool.iter().flat_map(|tx| tx.inputs.iter().map(|input| input.previous)).collect();
        if tx.inputs.iter().any(|input| spent.contains(&input.previous)) {
            return Err("spent in the pool".into());
        }
        self.world.pool.push(tx.clone());
        Ok(())
    }

    fn waiting(&self, id: &Hash) -> bool {
        self.world.pool.iter().any(|tx| tx.id() == *id)
    }

    fn now_s(&self) -> u64 {
        self.world.now_ms / 1_000
    }
}

pub fn coin() -> CoinConfig {
    let mut coin = CoinConfig::olc();
    coin.initial_difficulty = 8;
    coin.rules.coinbase_maturity = 1;
    coin.rules.retarget_interval = 0;
    coin.upgrade_from = 0;
    coin
}

pub fn folder(test: &str, name: &str) -> PathBuf {
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("channels").join(test).join(name);
    let _ = std::fs::remove_dir_all(&folder);
    folder
}

impl World {
    pub fn new() -> Self {
        let plugins = Plugins { consensus: Box::new(ProofOfWork), languages: olc_lang::all() };
        let miner = Wallet::from_secret([200; 32]).unwrap();
        Self { chain: Chain::new(coin(), plugins), pool: Vec::new(), now_ms: 100_000, miner }
    }

    /// Mines one block with whatever in the pool the chain takes, paying `to`.
    pub fn mine_to(&mut self, to: &Address) -> Vec<Hash> {
        let (good, _) = self.chain.recheck(std::mem::take(&mut self.pool));
        let time = self.chain.tip_header().time_ms + 1_000;
        let mut block = self.chain.block_template(to, &[], time);
        block.txs.extend(good.iter().cloned());
        block.header.merkle_root = merkle_root(&block.txs);
        assert!(ProofOfWork.try_seal(&mut block.header, 1_000_000));
        self.chain.add_block(block).expect("a block of checked payments");
        good.iter().map(Tx::id).collect()
    }

    pub fn mine(&mut self) -> Vec<Hash> {
        let to = self.miner.address();
        self.mine_to(&to)
    }

    pub fn party(&mut self, test: &str, name: &'static str, seed: u8) -> Party {
        let folder = folder(test, name);
        let wallet = Wallet::from_secret([seed; 32]).unwrap();
        let address = wallet.address();
        for _ in 0..3 {
            self.mine_to(&address);
        }
        let node = ChannelNode::open(&folder, name, write_new).unwrap();
        Party { name, node, wallet, folder, cut: false }
    }

    pub fn balance(&self, party: &Party) -> u64 {
        self.chain.balance(&party.wallet.address())
    }

    pub fn has(&self, id: &Hash) -> bool {
        self.chain.find_tx(id).is_some()
    }
}

impl Party {
    pub fn id(&self) -> NodeId {
        self.node.id
    }

    /// Runs an order against the chain.
    pub fn with<T>(&mut self, world: &mut World, order: impl FnOnce(&mut ChannelNode, &mut dyn Ground) -> T) -> T {
        let mut desk = Desk { world, wallet: &self.wallet };
        order(&mut self.node, &mut desk)
    }

    /// Opens the node again from its folder, as after a restart: only what it saved survives.
    pub fn restart(&mut self) {
        self.node = ChannelNode::open(&self.folder, self.name, write_new).unwrap();
    }
}

/// Passes every message waiting until none is left. Cut nodes neither send nor hear.
pub fn deliver(parties: &mut [Party], world: &mut World) {
    for _ in 0..200 {
        let mut moving: Vec<(usize, Out)> = Vec::new();
        for (index, party) in parties.iter_mut().enumerate() {
            while let Some(out) = party.node.take_out() {
                if !party.cut {
                    moving.push((index, out));
                }
            }
        }
        if moving.is_empty() {
            return;
        }
        for (from, out) in moving {
            for (index, party) in parties.iter_mut().enumerate() {
                if index == from || party.cut {
                    continue;
                }
                let bytes = match &out {
                    Out::To(to, bytes) if *to == party.node.id => bytes,
                    Out::All { bytes, except } if *except != Some(party.node.id) => bytes,
                    _ => continue,
                };
                let mut desk = Desk { world, wallet: &party.wallet };
                party.node.receive(bytes, &mut desk);
            }
        }
    }
    panic!("messages never stopped");
}

/// A second passes: every node not cut ticks, and messages move.
pub fn second(parties: &mut [Party], world: &mut World) {
    world.now_ms += 1_000;
    for party in parties.iter_mut().filter(|party| !party.cut) {
        let mut desk = Desk { world, wallet: &party.wallet };
        let now = desk.world.now_ms;
        party.node.tick(now, &mut desk);
    }
    deliver(parties, world);
}

pub fn seconds(parties: &mut [Party], world: &mut World, count: usize) {
    for _ in 0..count {
        second(parties, world);
    }
}

/// A block is mined, then a few seconds pass.
pub fn block(parties: &mut [Party], world: &mut World) -> Vec<Hash> {
    let mined = world.mine();
    seconds(parties, world, 2);
    mined
}

/// Opens a channel from `parties[from]` to `parties[to]` and mines until both see it open.
pub fn open(parties: &mut [Party], world: &mut World, from: usize, to: usize, amount: u64) -> Hash {
    let peer = parties[to].id();
    let id = parties[from].with(world, |node, ground| node.open_channel(peer, amount, DELAY, ground)).unwrap();
    seconds(parties, world, 2);
    for _ in 0..4 {
        block(parties, world);
    }
    let open = |party: &Party| party.node.channel(&id).is_some_and(|channel| channel.phase == olc_channel::Phase::Open);
    assert!(open(&parties[from]) && open(&parties[to]), "the channel opened on both sides");
    id
}
