//! Payments moving through channels, decided again from the files at every tick: a payment
//! offered to us is taken with its preimage, passed on along its route, or given back; what we
//! passed on settles the one it came from; turns start when updates wait. Because each decision
//! reads only saved state, a restart at any moment neither loses a payment nor sends one twice.
use crate::channel::{apply, Channel, Origin, Phase};
use crate::gossip::{BASE_FEE, DELTA, RATE};
use crate::node::{ChannelNode, Ground, PayState, Said};
use crate::wire::{ChannelId, Update};
use olc_crypto::{sha256, Hash};

/// Blocks the payee wants left before a payment's expiry.
pub const FINAL_DELTA: u16 = 18;
/// Blocks before an expiry at which a payment we can take is taken on the chain.
const CLAIM_MARGIN: u64 = 3;
/// Preimages remembered.
const MOST_PREIMAGES: usize = 4_096;

/// What to do with one payment offered to us.
enum Decision {
    Take(Hash),
    GiveBack,
    PassOn { out: ChannelId, update: Update },
    Wait,
}

fn fee_for(amount: u64) -> u64 {
    let rated = u128::from(amount) * u128::from(RATE) / 1_000_000;
    BASE_FEE.saturating_add(u64::try_from(rated).unwrap_or(u64::MAX))
}

impl ChannelNode {
    pub(crate) fn preimage_of(&self, hash: &Hash) -> Option<Hash> {
        self.book.preimages.iter().find(|(known, _)| known == hash).map(|(_, preimage)| *preimage)
    }

    /// Saves preimages in an offer before anything is signed for it, so a payment taken from us
    /// downstream is always claimed back upstream.
    pub(crate) fn learn_preimages(&mut self, channel: &Channel, updates: &[Update]) {
        let mut learned = false;
        for update in updates {
            let Update::Fulfil { preimage, .. } = update else {
                continue;
            };
            let hash = sha256(preimage);
            let offered = |channel: &Channel| channel.state.htlcs.iter().any(|htlc| htlc.ours && htlc.hash == hash);
            let ours = offered(channel) || self.channels.values().any(offered);
            if ours && self.preimage_of(&hash).is_none() {
                if self.book.preimages.len() >= MOST_PREIMAGES {
                    self.book.preimages.remove(0);
                }
                self.book.preimages.push((hash, *preimage));
                learned = true;
            }
        }
        if learned {
            self.save_book();
        }
    }

    /// True while a payment we offered can still be taken: in the channel, or on the chain until
    /// the channel is settled there.
    fn still_offered(&self, channel: &ChannelId, htlc: u64) -> bool {
        self.channels.get(channel).is_some_and(|channel| channel.phase != Phase::Closed && channel.offers(htlc))
    }

    fn origin_of(&self, from: (ChannelId, u64)) -> Option<(ChannelId, &Origin)> {
        self.channels.values().find_map(|channel| channel.origins.iter().find(|origin| origin.from == from).map(|origin| (channel.id, origin)))
    }

    fn decide(&mut self, channel: &Channel, at: usize, height: u64) -> Decision {
        let htlc = &channel.state.htlcs[at];
        if let Some(preimage) = self.preimage_of(&htlc.hash) {
            return Decision::Take(preimage);
        }
        if htlc.onward.is_empty() {
            let now = self.now_ms / 1_000;
            let made = self.book.invoices.iter().position(|made| made.invoice.hash == htlc.hash);
            let Some(index) = made else {
                return Decision::GiveBack;
            };
            let invoice = &self.book.invoices[index].invoice;
            let late = u64::from(htlc.expiry) < height + u64::from(invoice.final_delta) / 2;
            if htlc.amount < invoice.amount || late || (invoice.expired(now) && !self.book.invoices[index].paid) {
                return Decision::GiveBack;
            }
            let preimage = self.book.invoices[index].preimage;
            // Written down before the preimage leaves, so the request shows as paid either way.
            self.book.invoices[index].paid = true;
            self.save_book();
            self.say(Said::Received { hash: htlc.hash, amount: htlc.amount });
            return Decision::Take(preimage);
        }
        if let Some((out, origin)) = self.origin_of((channel.id, htlc.id)) {
            let still = self.still_offered(&out, origin.id);
            return if still { Decision::Wait } else { Decision::GiveBack };
        }
        let hop = &htlc.onward[0];
        let Some(out) = self.channels.values().find(|out| out.funding() == hop.channel && out.phase == Phase::Open) else {
            return Decision::GiveBack;
        };
        let pays = htlc.amount >= hop.amount.saturating_add(fee_for(hop.amount));
        let waits = u64::from(htlc.expiry) >= u64::from(hop.expiry) + u64::from(DELTA);
        let timely = u64::from(hop.expiry) > height + u64::from(FINAL_DELTA) / 2;
        if !pays || !waits || !timely || out.id == channel.id {
            return Decision::GiveBack;
        }
        let update = Update::Add { id: out.next_id, amount: hop.amount, hash: htlc.hash, expiry: hop.expiry, onward: htlc.onward[1..].to_vec() };
        let mut tried = out.queue.clone();
        tried.push(update.clone());
        if apply(&out.state, &tried, true, out.we_opened, out.terms.fee, height).is_err() {
            return Decision::GiveBack;
        }
        Decision::PassOn { out: out.id, update }
    }

    /// One pass over every channel's payments.
    pub(crate) fn settle(&mut self, ground: &mut dyn Ground) {
        let height = ground.height();
        let ids: Vec<ChannelId> = self.channels.keys().copied().collect();
        for id in &ids {
            let Some(channel) = self.channels.get(id).cloned().filter(|channel| channel.phase == Phase::Open) else {
                continue;
            };
            for at in 0..channel.state.htlcs.len() {
                let htlc = &channel.state.htlcs[at];
                if htlc.ours || channel.resolving(htlc.id) {
                    continue;
                }
                let (htlc_id, hash) = (htlc.id, htlc.hash);
                match self.decide(&channel, at, height) {
                    Decision::Wait => {}
                    Decision::Take(preimage) => self.queue_update(id, Update::Fulfil { id: htlc_id, preimage }),
                    Decision::GiveBack => self.queue_update(id, Update::Fail { id: htlc_id }),
                    Decision::PassOn { out, update } => {
                        let Some(mut next) = self.channels.get(&out).cloned() else {
                            continue;
                        };
                        let before = next.clone();
                        next.origins.push(Origin { id: next.next_id, hash, from: (*id, htlc_id) });
                        next.next_id += 1;
                        next.queue.push(update);
                        self.keep(Some(before), next, None);
                    }
                }
            }
        }
        self.start_turns(height);
        self.settle_payments();
        self.prune_origins();
        self.go_to_chain_in_time(height, ground);
    }

    fn queue_update(&mut self, id: &ChannelId, update: Update) {
        let Some(mut channel) = self.channels.get(id).cloned() else {
            return;
        };
        let before = channel.clone();
        channel.queue.push(update);
        self.keep(Some(before), channel, None);
    }

    fn start_turns(&mut self, height: u64) {
        let ready: Vec<ChannelId> = self
            .channels
            .values()
            .filter(|channel| channel.phase == Phase::Open && channel.round.is_none() && !channel.queue.is_empty())
            .map(|channel| channel.id)
            .collect();
        for id in ready {
            let Some(mut channel) = self.channels.get(&id).cloned() else {
                continue;
            };
            let before = channel.clone();
            let (wire, _dropped) = channel.propose(height);
            // A dropped offer is no longer offered: its payment fails and what it came from is
            // given back at the next pass, both read from the saved channel.
            self.keep(Some(before), channel, wire);
        }
    }

    fn settle_payments(&mut self) {
        let mut said = Vec::new();
        for at in 0..self.book.payments.len() {
            let payment = &self.book.payments[at];
            if payment.state != PayState::Pending {
                continue;
            }
            let state = if self.preimage_of(&payment.hash).is_some() {
                PayState::Paid
            } else if self.still_offered(&payment.channel, payment.htlc) {
                continue;
            } else {
                PayState::Failed
            };
            said.push(match state {
                PayState::Paid => Said::Paid { hash: payment.hash, amount: payment.amount },
                _ => Said::PayFailed { hash: payment.hash },
            });
            self.book.payments[at].state = state;
        }
        if !said.is_empty() {
            self.save_book();
            said.into_iter().for_each(|said| self.say(said));
        }
    }

    /// Forgets links whose payment on both sides is settled.
    fn prune_origins(&mut self) {
        let mut done: Vec<(ChannelId, u64)> = Vec::new();
        for channel in self.channels.values() {
            for origin in &channel.origins {
                let from_open = self.channels.get(&origin.from.0).is_some_and(|from| from.state.htlcs.iter().any(|htlc| !htlc.ours && htlc.id == origin.from.1));
                if !from_open && !channel.offers(origin.id) {
                    done.push((channel.id, origin.id));
                }
            }
        }
        for (id, origin) in done {
            if let Some(mut channel) = self.channels.get(&id).cloned() {
                let before = channel.clone();
                channel.origins.retain(|kept| kept.id != origin);
                self.keep(Some(before), channel, None);
            }
        }
    }

    /// A payment we offered past its expiry, or one we can take close to its own, goes to the
    /// chain while there is time: the other side has stopped answering.
    fn go_to_chain_in_time(&mut self, height: u64, ground: &mut dyn Ground) {
        let late: Vec<ChannelId> = self
            .channels
            .values()
            .filter(|channel| channel.phase == Phase::Open)
            .filter(|channel| {
                channel.state.htlcs.iter().any(|htlc| {
                    let expiry = u64::from(htlc.expiry);
                    match htlc.ours {
                        true => height >= expiry,
                        false => self.preimage_of(&htlc.hash).is_some() && height + CLAIM_MARGIN >= expiry,
                    }
                })
            })
            .map(|channel| channel.id)
            .collect();
        for id in late {
            let _ = self.force_close(&id, ground);
        }
    }
}
