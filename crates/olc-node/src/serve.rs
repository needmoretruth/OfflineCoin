//! What a full node gives a light one, and how the one [`Engine`] becomes light when its owner
//! chooses that. Nothing else about a node changes: same links, same keys, same commands.
use crate::engine::{Engine, PeerId};
use crate::event::{Outgoing, Status};
use crate::light::Light;
use crate::message::Message;
use alloc::boxed::Box;
use alloc::vec::Vec;
use olc_core::proof::Coins;
use olc_core::{Address, BlockHeader, CoinProof, Hash, OutPoint, Tx};
use olc_wallet::{prepare, Order, Unsigned, WalletError};

/// A page of headers. A small radio queue on a slow reader sets this, as it does for blocks.
const HEADERS_AT_ONCE: usize = 16;
const COINS_AT_ONCE: usize = 8;

impl Engine {
    /// From now on this node holds headers only and never mines. Blocks it held stay on disk.
    pub fn become_light(&mut self, most_headers: usize, saved: &[BlockHeader]) {
        let mut light = Light::new(&self.chain, most_headers);
        light.restore(saved, &self.chain);
        self.light = Some(Box::new(light));
        self.candidate = None;
    }

    pub fn light(&self) -> Option<&Light> {
        self.light.as_deref()
    }

    /// Where this node's wallet finds its coins.
    pub fn coins(&self) -> &dyn Coins {
        match self.light.as_deref() {
            Some(light) => light,
            None => &self.chain,
        }
    }

    /// Follows `address` without any key for it. Its coins can be seen, never spent.
    pub fn watch(&mut self, address: Address) {
        if !self.watched.contains(&address) {
            self.watched.push(address);
        }
        if let Some(light) = self.light.as_mut() {
            light.refresh();
        }
    }

    pub fn watched(&self) -> Vec<(Address, u64)> {
        let balance = |address: &Address| (*address, self.coins().balance(address));
        self.watched.iter().map(balance).collect()
    }

    /// Builds a payment from the watched addresses, for a wallet elsewhere to sign.
    pub fn prepare(&self, to: Address, amount: u64, fee: u64) -> Result<Unsigned, WalletError> {
        let change = *self.watched.first().ok_or(WalletError::NoSuchKey)?;
        let payments = [(to, amount)];
        let order = Order { payments: &payments, data: &[], fee, change };
        prepare(self.coins(), &self.watched, &order, &[])
    }

    /// A light node's turn. False on a node that holds blocks.
    pub(crate) fn light_tick(&mut self, device_ms: u64) -> bool {
        let Some(light) = self.light.as_mut() else {
            return false;
        };
        light.tick(device_ms, &self.wallet, &self.watched);
        while let Some(outgoing) = light.take_outgoing() {
            self.outbox.push_back(outgoing);
        }
        true
    }

    /// True when a light node took the message.
    pub(crate) fn light_heard(&mut self, from: PeerId, message: &Message) -> bool {
        let Some(light) = self.light.as_mut() else {
            return false;
        };
        light.on_message(from, message.clone(), &self.chain);
        true
    }

    pub(crate) fn light_status(&self, full: Status) -> Status {
        let Some(light) = self.light.as_deref() else {
            return full;
        };
        let headers = light.headers();
        Status {
            height: headers.height(),
            tip: headers.tip().id(),
            work: headers.work(),
            difficulty: headers.tip().difficulty,
            balance: self.wallet.balance(light),
            peers: light.peers(),
            syncing_to: light.syncing_to(),
            ..full
        }
    }

    /// Sends what this wallet signed. A light node has nothing to check it against.
    pub fn send_signed(&mut self, tx: Tx) -> Hash {
        match self.light.as_mut() {
            Some(light) => {
                let id = tx.id();
                light.spending(&tx);
                let message = Message::Tx(tx);
                self.outbox.push_back(Outgoing::All { except: None, message });
                id
            }
            None => self.send_own(tx),
        }
    }

    pub(crate) fn serve_headers(&mut self, from: PeerId, named: &[(u64, Hash)]) {
        let reply = match self.chain.headers_after(named, HEADERS_AT_ONCE) {
            Ok(headers) => Message::Headers(headers),
            Err(oldest) => Message::Pruned { oldest },
        };
        self.outbox.push_back(Outgoing::To(from, reply));
    }

    /// One page of the coins of `address`. A coin whose block is gone cannot be proven, so the
    /// peer hears that instead.
    pub(crate) fn serve_coins(
        &mut self,
        from: PeerId,
        address: Address,
        after: Option<OutPoint>,
    ) {
        let coins = self.chain.unspent_of(&address);
        let left = coins.iter().filter(|(point, _)| after.is_none_or(|after| *point > after));
        let page: Vec<_> = left.take(COINS_AT_ONCE + 1).collect();
        let more = page.len() > COINS_AT_ONCE;
        let mut last = None;
        for (point, coin) in page.into_iter().take(COINS_AT_ONCE) {
            let block = self.chain.blocks_from(coin.height, 1).pop();
            let Some(proof) = block.and_then(|block| CoinProof::of(&block, coin.height, point))
            else {
                let oldest = self.chain.oldest_held();
                self.outbox.push_back(Outgoing::To(from, Message::Pruned { oldest }));
                return;
            };
            last = Some(*point);
            self.outbox.push_back(Outgoing::To(from, Message::Coin(Box::new(proof))));
        }
        let end = Message::CoinsEnd { address, last, more };
        self.outbox.push_back(Outgoing::To(from, end));
    }
}
