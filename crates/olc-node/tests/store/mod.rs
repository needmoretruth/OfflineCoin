//! Flash as it really behaves, for tests: bits only fall on write, an erase takes a whole sector,
//! and a power cut can land inside either. Everything above this is the shipped code, fed real
//! mined blocks with real signatures.
#![allow(dead_code)]

use olc_core::medium::{Keep, Medium, SECTOR};
use olc_core::{Address, Block, Chain, CoinConfig, Consensus, Plugins, Tx};
use olc_pow::ProofOfWork;
use olc_wallet::Wallet;
use std::sync::{Arc, Mutex};

pub struct Nor {
    pub bytes: Vec<u8>,
    pub steps: u64,
    /// The step that loses power.
    pub cut_at: Option<u64>,
    pub dead: bool,
    /// Erases each sector took, which is what wears flash out.
    pub erases: Vec<u32>,
}

pub type Flash = Arc<Mutex<Nor>>;

pub fn flash(size: usize) -> Flash {
    Arc::new(Mutex::new(Nor {
        bytes: vec![0xFF; size],
        steps: 0,
        cut_at: None,
        dead: false,
        erases: vec![0; size / SECTOR as usize],
    }))
}

/// A copy of what the flash held when the power went, with the power back on.
pub fn powered_again(flash: &Flash) -> Flash {
    let bytes = flash.lock().unwrap().bytes.clone();
    let sectors = bytes.len() / SECTOR as usize;
    Arc::new(Mutex::new(Nor {
        bytes,
        steps: 0,
        cut_at: None,
        dead: false,
        erases: vec![0; sectors],
    }))
}

impl Nor {
    /// True when this step finishes. A cut step does part of its work first.
    fn step(&mut self) -> Option<u64> {
        if self.dead {
            return Some(0);
        }
        self.steps += 1;
        if self.cut_at == Some(self.steps) {
            self.dead = true;
            return Some(self.steps);
        }
        None
    }
}

pub struct Region {
    flash: Flash,
    base: usize,
    size: usize,
}

impl Region {
    fn place(&self, at: u64) -> usize {
        self.base + (at % self.size as u64) as usize
    }
}

impl Medium for Region {
    fn capacity(&self) -> u64 {
        self.size as u64
    }

    fn read(&self, at: u64, bytes: &mut [u8]) -> bool {
        let flash = self.flash.lock().unwrap();
        let start = self.place(at);
        let first = bytes.len().min(self.base + self.size - start);
        let (before, after) = bytes.split_at_mut(first);
        before.copy_from_slice(&flash.bytes[start..start + first]);
        after.copy_from_slice(&flash.bytes[self.base..self.base + after.len()]);
        true
    }

    fn write(&mut self, at: u64, bytes: &[u8]) -> bool {
        let places: Vec<usize> = (0..bytes.len()).map(|i| self.place(at + i as u64)).collect();
        let mut flash = self.flash.lock().unwrap();
        let cut = flash.step();
        // A cut write leaves some of the record, and the last word only half written.
        let written = match cut {
            None => bytes.len(),
            Some(0) => return false,
            Some(step) => (step as usize * 37) % (bytes.len() + 1),
        };
        for (place, byte) in places.iter().zip(bytes).take(written) {
            flash.bytes[*place] &= byte;
        }
        if cut.is_some() {
            if let (Some(place), Some(byte)) = (places.get(written), bytes.get(written)) {
                flash.bytes[*place] &= byte | 0xF0;
            }
            return false;
        }
        true
    }

    fn erase(&mut self, at: u64) -> bool {
        let start = self.place(at);
        let mut flash = self.flash.lock().unwrap();
        let cut = flash.step();
        flash.erases[start / SECTOR as usize] += 1;
        let cleared = match cut {
            None => SECTOR as usize,
            Some(0) => return false,
            Some(step) => (step as usize * 61) % SECTOR as usize,
        };
        flash.bytes[start..start + cleared].fill(0xFF);
        cut.is_none()
    }
}

pub const STATE_SIZE: usize = 256 * 1024;

/// The way a board cuts its flash: two places for the state, the rest for the log.
pub fn keep_on(flash: &Flash, recent: Option<u64>) -> Keep {
    let size = flash.lock().unwrap().bytes.len();
    let region = |base, size| -> Box<dyn Medium> {
        Box::new(Region {
            flash: flash.clone(),
            base,
            size,
        })
    };
    Keep {
        log: region(STATE_SIZE * 2, size - STATE_SIZE * 2),
        states: [region(0, STATE_SIZE), region(STATE_SIZE, STATE_SIZE)],
        shelf: None,
        recent,
    }
}

pub fn coin() -> CoinConfig {
    let mut config = CoinConfig::olc();
    config.initial_difficulty = 8;
    config.rules.min_difficulty = 1;
    config.rules.coinbase_maturity = 2;
    config.rules.retarget_interval = 10;
    config
}

pub fn plugins() -> Plugins {
    Plugins {
        consensus: Box::new(ProofOfWork),
        languages: olc_lang::all(),
    }
}

pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub fn below(&mut self, limit: u64) -> u64 {
        self.next() % limit
    }
}

pub fn mined(chain: &Chain, miner: &Address, txs: &[Tx], offset_ms: u64) -> Block {
    let time = chain.tip_header().time_ms + 1_000 + offset_ms;
    let mut block = chain.block_template(miner, txs, time);
    assert!(ProofOfWork.try_seal(&mut block.header, 10_000_000));
    block
}

/// A real chain with forks: two miners working apart, paying each other, then hearing each other.
pub fn forking_blocks(rounds: u64, seed: u64) -> Vec<Block> {
    let mut rng = Rng(seed | 1);
    let wallets = [
        Wallet::from_secret([1; 32]).unwrap(),
        Wallet::from_secret([2; 32]).unwrap(),
    ];
    let mut sides = [Chain::new(coin(), plugins()), Chain::new(coin(), plugins())];
    let mut script = Vec::new();
    for _ in 0..rounds {
        let mut fresh: [Vec<Block>; 2] = [Vec::new(), Vec::new()];
        for side in 0..2 {
            for _ in 0..rng.below(5) {
                let chain = &sides[side];
                let wallet = &wallets[side];
                let other = wallets[1 - side].address();
                let spendable = chain.height() > 4 && rng.below(3) == 0;
                let pay = spendable.then(|| wallet.pay(chain, other, 7_000 + rng.below(9), 0, &[]));
                let txs: Vec<Tx> = pay.into_iter().flatten().collect();
                let block = mined(chain, &wallet.address(), &txs, rng.below(40_000));
                sides[side].add_block(block.clone()).unwrap();
                fresh[side].push(block);
            }
        }
        for side in 0..2 {
            for block in &fresh[side] {
                let _ = sides[1 - side].add_block(block.clone());
            }
        }
        let [first, second] = fresh;
        script.extend(first);
        script.extend(second);
    }
    script
}

/// Rebuilds the unspent set from nothing over `chain`'s best blocks.
pub fn replayed(chain: &Chain) -> Chain {
    let mut full = Chain::new(chain.config().clone(), plugins());
    for block in chain.best_chain_from(1) {
        full.add_block(block).unwrap();
    }
    full
}
