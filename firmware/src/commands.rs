//! Keys, coin and chain commands. A secret key only ever leaves through `println!`, which is the
//! cable: the radio gate never lets a `key` line in, and `say` is never used on one.
use crate::node::{decode_hash, hex, Node};
use olc_crypto::Signer;
use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use esp_hal::rng::Rng;
use esp_println::println;
use olc_core::amount::parse_coins;
use olc_control::line::{
    name_in, wifi_in, FEWEST_WIFI_PASSWORD_BYTES, MOST_NAME_BYTES, MOST_WIFI_NAME_BYTES,
    MOST_WIFI_PASSWORD_BYTES,
};

use olc_core::{BlockHeader, CoinConfig, Hash, Opened};
use olc_wallet::{format_address, parse_address};

/// Shares held between two questions, so an answer stays small.
const MOST_SHARES: usize = 8;

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16).ok())
        .collect()
}

pub fn note_bytes(text: &str) -> Vec<u8> {
    unhex(text).unwrap_or_default()
}

pub fn opened_name(opened: Opened) -> &'static str {
    match opened {
        Opened::New => "new",
        Opened::Resumed { .. } => "resumed",
        Opened::Rebuilt { .. } => "rebuilt",
        Opened::Lost => "lost",
    }
}

/// A block someone else builds, ground here on their terms.
pub struct Work {
    pub header: BlockHeader,
    pub difficulty: u64,
    /// Headers that reached the difficulty, until someone asks for them.
    pub found: VecDeque<BlockHeader>,
}

impl Node {
    pub fn pay(&mut self, address: &str, amount: &str) {
        let (Some(to), Some(units)) = (parse_address(address), parse_coins(amount)) else {
            self.say("error pay ADDRESS AMOUNT".into());
            return;
        };
        let started = crate::node::device_ms();
        let fee = self.runner.engine.rules().min_fee;
        match self.runner.engine.pay(to, units, fee) {
            Ok(id) => self.say(format!("paid {} in {}ms", hex(&id), crate::node::device_ms() - started)),
            Err(error) => self.say(format!("error {error:?}")),
        }
    }

    fn name_from(&mut self, line: &str) {
        match name_in(line) {
            Some(name) if name.len() <= MOST_NAME_BYTES => {
                self.rename(&name);
                self.say("named".into());
            }
            Some(_) => self.say(format!("error name has {MOST_NAME_BYTES} bytes at most")),
            None => self.say("error name NAME".into()),
        }
    }

    fn wifi_from(&mut self, line: &str) {
        let Some((ssid, password)) = wifi_in(line) else {
            return self.say("error wifi \"NAME\" \"PASSWORD\"".into());
        };
        if ssid.len() > MOST_WIFI_NAME_BYTES {
            return self.say(format!("error wifi name has {MOST_WIFI_NAME_BYTES} bytes at most"));
        }
        let fits = FEWEST_WIFI_PASSWORD_BYTES..=MOST_WIFI_PASSWORD_BYTES;
        if !password.is_empty() && !fits.contains(&password.len()) {
            return self.say(format!(
                "error wifi password has {FEWEST_WIFI_PASSWORD_BYTES} to {MOST_WIFI_PASSWORD_BYTES} bytes"
            ));
        }
        self.restart_with(|settings| {
            settings.ssid = ssid;
            settings.password = password;
        });
    }

    /// Runs `line` when it is one of these commands.
    pub fn run_chain_command(&mut self, line: &str) -> bool {
        let mut words = line.split_whitespace();
        let rest = |word: &str| line.trim_start().strip_prefix(word).unwrap_or_default().trim();
        match (words.next(), words.next(), words.next()) {
            (Some("name"), _, _) => self.name_from(line),
            (Some("wifi"), Some("off"), _) => self.restart_with(|settings| {
                settings.ssid.clear();
                settings.password.clear();
            }),
            (Some("wifi"), _, _) => self.wifi_from(line),
            (Some("keys"), _, _) => self.print_keys(),
            (Some("key"), Some("new"), _) => self.new_key(),
            (Some("key"), Some("use"), Some(index)) => self.choose_key(index),
            (Some("key"), Some("import"), Some(text)) => self.import_key(text),
            (Some("key"), Some("export"), Some(index)) => self.export_key(index),
            (Some("mineto"), Some(target), _) => self.mine_to(target),
            (Some("keep"), Some(blocks), _) => self.keep_blocks(blocks),
            (Some("light"), Some(state), _) => {
                let on = state == "on";
                self.say(format!("light {on}"));
                self.restart_with(|settings| settings.light = if on { "on" } else { "" }.into());
            }
            (Some("merge"), Some(state), _) => {
                let on = state == "on";
                self.runner.engine.set_merging(on);
                self.save_setting(|settings| settings.merge = if on { "" } else { "off" }.into());
                self.say(format!("merge {on}"));
            }
            (Some("note"), Some(_), _) => {
                let result = self.runner.engine.write_note(rest("note").as_bytes());
                match result {
                    Ok(id) => self.say(format!("noted {}", hex(&id))),
                    Err(error) => self.say(format!("error {error:?}")),
                }
            }
            (Some("minenote"), Some("off"), _) => self.mine_note(""),
            (Some("minenote"), Some(_), _) => self.mine_note(rest("minenote")),
            (Some("snapshot"), Some("accept"), _) => self.accept_snapshot(),
            (Some("coin"), None, _) => self.print_coin(),
            (Some("coin"), Some("confirm"), _) => self.confirm_coin(),
            (Some("coin"), Some("default"), _) => self.change_coin(CoinConfig::olc()),
            (Some("coin"), Some(text), _) => {
                let coin = unhex(text).and_then(|bytes| CoinConfig::from_bytes(&bytes));
                match coin {
                    Some(coin) => self.change_coin(coin),
                    None => self.say("error coin HEX".into()),
                }
            }
            (Some("work"), Some("stop"), _) => {
                self.work = None;
                self.say("work stopped".into());
            }
            (Some("work"), Some("shares"), _) => self.print_shares(),
            (Some("work"), Some(header), Some(difficulty)) => self.take_work(header, difficulty),
            _ => return false,
        }
        true
    }

    fn print_keys(&mut self) {
        let wallet = self.runner.engine.wallet();
        let receiving = wallet.receiving();
        let lines: Vec<String> = wallet
            .keys()
            .enumerate()
            .map(|(index, (address, public))| {
                let mark = if index == receiving { " receiving" } else { "" };
                format!(
                    "key {index} {} {}{mark}",
                    format_address(&address),
                    hex(&public)
                )
            })
            .collect();
        for line in lines {
            self.say(line);
        }
    }

    fn save_keys(&mut self) {
        let wallet = self.runner.engine.wallet();
        let secrets: Vec<[u8; 32]> = (0..wallet.key_count())
            .filter_map(|index| wallet.export_secret(index))
            .collect();
        let receiving = wallet.receiving();
        if !self.store.save_keys(&secrets, receiving) {
            self.say("error keys not saved".into());
        }
    }

    fn add_key(&mut self, secret: [u8; 32]) {
        match self.runner.engine.wallet_mut().add_secret(secret) {
            Ok(index) => {
                self.save_keys();
                let address = self.runner.engine.wallet().keys().nth(index);
                if let Some((address, _)) = address {
                    self.say(format!("key {index} {}", format_address(&address)));
                }
            }
            Err(error) => self.say(format!("error {error:?}")),
        }
    }

    fn new_key(&mut self) {
        // The radio is on, so this is true hardware randomness.
        let mut secret = [0u8; 32];
        Rng::new().read(&mut secret);
        self.add_key(secret);
    }

    fn import_key(&mut self, text: &str) {
        let mut secret: Hash = [0; 32];
        if decode_hash(text, &mut secret) {
            self.add_key(secret);
        } else {
            self.say("error key import HEX".into());
        }
    }

    fn export_key(&mut self, index: &str) {
        let secret = index
            .parse()
            .ok()
            .and_then(|index| self.runner.engine.wallet().export_secret(index));
        match secret {
            // The cable only, and never through `say`.
            Some(secret) => println!("secret {index} {}", hex(&secret)),
            None => println!("error key export INDEX"),
        }
    }

    fn choose_key(&mut self, index: &str) {
        let chosen = index
            .parse()
            .ok()
            .is_some_and(|index| self.runner.engine.wallet_mut().set_receiving(index).is_ok());
        if chosen {
            self.save_keys();
            // Beacons are signed by the key behind the address they show.
            let wallet = self.runner.engine.wallet();
            if let Some(secret) = wallet.export_secret(wallet.receiving()) {
                if let Some(signer) = Signer::new(&secret) {
                    self.control.signer = signer;
                }
            }
            self.say(format!("receiving {index}"));
        } else {
            self.say("error key use INDEX".into());
        }
    }

    fn mine_to(&mut self, target: &str) {
        let miner = parse_address(target);
        if target == "own" || miner.is_some() {
            self.runner.engine.set_miner(miner);
            let saved = if miner.is_some() { target } else { "" };
            self.save_setting(|settings| settings.miner = saved.into());
            let address = self.runner.engine.miner();
            self.say(format!("mining to {}", format_address(&address)));
        } else {
            self.say("error mineto ADDRESS|own".into());
        }
    }

    fn keep_blocks(&mut self, blocks: &str) {
        let recent = blocks.parse().ok();
        if blocks != "all" && recent.is_none() {
            self.say("error keep BLOCKS|all".into());
            return;
        }
        let chain = self.runner.engine.chain_mut();
        chain.set_recent(recent);
        let kept = chain.held().recent;
        let saved = kept.map(|blocks| format!("{blocks}")).unwrap_or_default();
        self.save_setting(|settings| settings.recent = saved);
        self.say(format!("keep {kept:?}"));
    }

    fn mine_note(&mut self, text: &str) {
        self.runner.engine.set_note(text.as_bytes());
        let saved = hex(self.runner.engine.note());
        self.save_setting(|settings| settings.note = saved);
        self.say(format!("mine note {} bytes", text.len()));
    }

    fn accept_snapshot(&mut self) {
        match self.runner.engine.status().needs_history {
            Some((peer, _)) => {
                self.runner.engine.accept_snapshot(peer);
                self.say(format!("snapshot trusting peer {peer}"));
            }
            None => self.say("error no peer to trust".into()),
        }
    }

    fn print_coin(&mut self) {
        let chain = self.runner.engine.chain();
        let coin = chain.config();
        let line = format!(
            "coin {} {} {}",
            coin.name,
            coin.ticker,
            hex(&chain.genesis_id())
        );
        self.say(line);
    }

    /// One coin's blocks sit in the log, so a different coin asks first. The keys stay.
    fn change_coin(&mut self, coin: CoinConfig) {
        let chain = self.runner.engine.chain();
        if coin.genesis().id() == chain.genesis_id() {
            self.say("coin already running".into());
        } else if chain.height() == 0 {
            self.switch_to(&coin);
        } else {
            let line = format!(
                "coin asks confirm: {} blocks of {} go, keys stay",
                chain.height(),
                chain.config().name
            );
            self.waiting_coin = Some(coin);
            self.say(line);
        }
    }

    fn confirm_coin(&mut self) {
        match self.waiting_coin.take() {
            Some(coin) => self.switch_to(&coin),
            None => self.say("error no coin waiting".into()),
        }
    }

    /// A log written for another coin fails its checks, so the new coin starts clean.
    fn switch_to(&mut self, coin: &CoinConfig) {
        if self.store.save_coin(coin) {
            esp_hal::system::software_reset();
        }
        self.say("error coin not saved".into());
    }

    fn take_work(&mut self, header: &str, difficulty: &str) {
        let header = unhex(header).and_then(|bytes| BlockHeader::decode(&bytes));
        match (header, difficulty.parse()) {
            (Some(header), Ok(difficulty)) => {
                let found = self.work.take().map(|work| work.found).unwrap_or_default();
                self.work = Some(Work {
                    header,
                    difficulty,
                    found,
                });
                self.say("work taken".into());
            }
            _ => self.say("error work HEADER DIFFICULTY".into()),
        }
    }

    fn print_shares(&mut self) {
        let found = self.work.as_mut().map(|work| core::mem::take(&mut work.found));
        let words: Vec<String> = found
            .unwrap_or_default()
            .iter()
            .map(|header| hex(&header.encode()))
            .collect();
        self.say(format!("shares {}", words.join(" ")));
    }

    /// Tries a batch of the work in hand and keeps each share found for whoever asks.
    pub fn grind(&mut self, batch: u32) {
        let Some(work) = &mut self.work else {
            return;
        };
        if self
            .runner
            .engine
            .grind(&mut work.header, work.difficulty, batch)
        {
            if work.found.len() >= MOST_SHARES {
                work.found.pop_front();
            }
            work.found.push_back(work.header);
            work.header.nonce = work.header.nonce.wrapping_add(1);
        }
    }
}
