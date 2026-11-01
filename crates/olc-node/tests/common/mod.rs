//! Small networks of engines for the tests in this folder.
#![allow(dead_code)]

use olc_core::{CoinConfig, Plugins};
use olc_node::{Engine, Outgoing};
use olc_pow::ProofOfWork;
use olc_wallet::Wallet;

pub fn engine(seed: u8) -> Engine {
    engine_with_maturity(seed, 1)
}

pub fn engine_with_maturity(seed: u8, maturity: u64) -> Engine {
    let mut config = CoinConfig::olc();
    config.initial_difficulty = 8;
    config.rules.coinbase_maturity = maturity;
    config.rules.retarget_interval = 0;
    let plugins = Plugins {
        consensus: Box::new(ProofOfWork),
        languages: olc_lang::all(),
    };
    Engine::new(config, plugins, Wallet::from_secret([seed; 32]).unwrap())
}

/// Carries messages between `nodes` until all are quiet. A node's peer id is its index.
pub fn deliver(nodes: &mut [Engine]) {
    let mut moved = true;
    while moved {
        moved = false;
        for sender in 0..nodes.len() {
            while let Some(outgoing) = nodes[sender].take_outgoing() {
                moved = true;
                let (to, except, message) = match outgoing {
                    Outgoing::To(peer, message) => (Some(peer as usize), None, message),
                    Outgoing::All { except, message } => {
                        (None, except.map(|peer| peer as usize), message)
                    }
                };
                for receiver in 0..nodes.len() {
                    let chosen = to.is_none_or(|peer| peer == receiver);
                    if receiver != sender && chosen && except != Some(receiver) {
                        nodes[receiver].on_message(sender as u32, message.clone());
                    }
                }
            }
        }
    }
}

pub fn silence(node: &mut Engine) {
    while node.take_outgoing().is_some() {}
}

pub fn mine(node: &mut Engine, now: u64) {
    assert!(node.mine(1_000_000, now));
}
