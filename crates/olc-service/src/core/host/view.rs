//! What the window sees of the node, built a few times a second from the engine and the state the
//! node's thread keeps.
use super::super::explorer::{recent_blocks, text_of};
use super::super::history::{Activity, ActivityKind, RECENT};
use super::super::host_parts::{stored_bytes, watched_view};
use super::super::radios;
use super::super::snapshot::{KeyView, NodeView};
use super::Worker;
use olc_core::{Address, Opened};
use olc_node::{Light, Runner};
use olc_wallet::format_address;
use std::sync::atomic::Ordering;

impl Worker {
    pub(super) fn publish(&mut self, runner: &mut Runner) {
        let engine = &runner.engine;
        let chain = engine.chain();
        let status = engine.status();
        let wallet = engine.wallet();
        let own: Vec<Address> = wallet.keys().map(|(address, _)| address).collect();
        self.history.follow(chain, &own);
        if status.tip != self.last_tip {
            self.last_tip = status.tip;
            self.tips += 1;
            self.blocks = recent_blocks(chain);
        }
        let now_ms = super::super::host_parts::unix_ms();
        self.watch.follow(chain, now_ms);
        let other_tips = self.state.other_tips.lock().map(|tips| tips.clone()).unwrap_or_default();
        self.watch.look_for_rivals(chain, &other_tips, now_ms);
        self.save_light(engine, &status.tip);
        let coins = engine.coins();
        let next = coins.height() + 1;
        let mine: Vec<_> = own.iter().flat_map(|address| coins.unspent_of(address)).collect();
        let maturing = mine.iter().filter(|(_, entry)| !coins.is_mature(entry, next)).map(|(_, entry)| entry.output.value).sum();
        // The coins a payment picks from, so the figure never promises more than a payment finds.
        let spendable = engine.spendable();
        let receiving = wallet.receiving();
        let keys = wallet.keys().enumerate().map(|(index, (address, public))| KeyView {
            address: format_address(&address),
            public: public.iter().map(|byte| format!("{byte:02x}")).collect(),
            receiving: index == receiving,
        });
        let held = chain.held();
        let pool = self.history.pool(chain, engine.pool(), &own, super::super::host_parts::unix_ms());
        let mut activity = pool.mine;
        // A payment this app sent from coins under a lock spends none of the wallet's own coins,
        // so the pool's shapes miss it; the list kept when it was sent still names it.
        if let Ok(mut waiting) = self.state.waiting.lock() {
            waiting.retain(|item| engine.in_pool(&item.id));
            let missed: Vec<Activity> = waiting
                .iter()
                .rev()
                .filter(|item| !activity.iter().any(|seen| seen.id == item.id))
                .map(|item| Activity { amount: item.amount, time_ms: item.time_ms, ..Activity::waiting(ActivityKind::Sent, item.id) })
                .collect();
            activity.extend(missed);
        }
        activity.extend(self.history.items().take(RECENT).cloned());
        let wallet_part = super::super::host_wallet::wallet_view(engine, &activity, &self.history, pool.all);
        let view = NodeView {
            coin: Some(chain.config().clone()),
            genesis: chain.genesis_id(),
            tip: status.tip,
            height: status.height,
            work: status.work,
            difficulty: status.difficulty,
            keeps: engine.keeps(),
            peers: runner.neighbour_count(),
            pool: status.pool,
            hashrate: self.state.hashrate.load(Ordering::Relaxed),
            cap: self.state.cap.load(Ordering::Relaxed),
            mining: self.state.mining.load(Ordering::Relaxed) && engine.light().is_none(),
            light: engine.light().map(Light::status),
            watched: watched_view(engine),
            syncing_to: status.syncing_to,
            loading: false,
            balance: status.balance,
            spendable,
            maturing,
            address: format_address(&engine.address()),
            activity,
            blocks_mined: self.history.blocks_mined(),
            port: self.port,
            tip_changes: self.tips,
            stored_bytes: stored_bytes(engine, held.bytes),
            storage_full: held.oldest > 0,
            keys: keys.collect(),
            miner: format_address(&engine.miner()),
            miner_is_chosen: engine.miner_is_chosen(),
            mine_note: text_of(engine.note()),
            merging: engine.merging(),
            version_one: engine.version_one(),
            links: runner.rank(),
            bluetooth: radios::bluetooth_here().then_some(self.wants_bluetooth),
            no_bluetooth: radios::no_bluetooth(),
            oldest: held.oldest,
            recent: held.recent,
            fits: held.fits,
            unspent: chain.unspent_count(),
            needs_history: status.needs_history,
            assumed_from: status.assumed_from,
            fetching: status.fetching,
            rebuilt: matches!(chain.opened(), Opened::Rebuilt { .. } | Opened::Lost),
            blocks: self.blocks.clone(),
            chain: self.watch.view(),
            notes: self.history.notes().cloned().collect(),
            explored: self.history.enrich(self.state.explored.lock().map(|x| x.clone()).unwrap_or_default()),
            locks: engine.locked_coins(),
            half_spend: self.state.half_spend.lock().map(|x| x.clone()).unwrap_or_default(),
            wallet: wallet_part,
            channels: super::super::host_channels::view_of(&self.state),
        };
        runner.card.mining = view.mining;
        runner.card.hashrate = view.hashrate;
        if let Ok(known) = self.state.known.lock() {
            runner.card.known.clone_from(&known);
        }
        if let Ok(mut slot) = self.state.view.lock() {
            *slot = view;
        }
    }
}
