//! Sorting what was found. A different genesis is a different coin. The same genesis with
//! different rules is a split chain. Both the same is one network, even if its nodes compete.
use super::Reach;
use olc_core::{CoinConfig, Hash, Rules};
use olc_node::Keeps;
use std::collections::{BTreeMap, BTreeSet};

/// The coin something runs, when it has said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub coin: CoinConfig,
    pub genesis: Hash,
}

impl Identity {
    pub fn of(coin: CoinConfig) -> Self {
        let genesis = coin.genesis().id();
        Self { coin, genesis }
    }
}

/// What it is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Work {
    Mining(u64),
    /// Mining slower, at this rate, until its chip cools.
    Cooling(u64),
    Idle,
    Offline,
    Silent,
    Restarting,
}

/// One thing found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub reach: Reach,
    /// `host:port`, for what a socket can reach.
    pub address: Option<String>,
    /// Who told this app about it.
    pub via: Option<String>,
    pub device: String,
    pub name: String,
    pub identity: Option<Identity>,
    pub tip: Option<Hash>,
    pub height: Option<u64>,
    pub work_done: Option<u128>,
    pub build: String,
    pub work: Work,
    pub keeps: Keeps,
    /// How old the row is, when it comes only from a past answer.
    pub age_secs: Option<u64>,
}

/// How a chain stands to the one this app runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relation {
    /// This app runs on it.
    Yours,
    /// Same coin and rules as nothing this app runs: a network it could join.
    Network,
    /// Same genesis as another chain listed, but other rules.
    Split,
    /// Nothing said which coin.
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChainGroup {
    pub rules: Option<Rules>,
    /// The coin this chain runs, which a person joins. A split has a coin of its own.
    pub coin: Option<CoinConfig>,
    pub relation: Relation,
    pub entries: Vec<Entry>,
    /// Different best blocks among its nodes, so at least this many are competing or behind.
    pub tips: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoinGroup {
    /// `None` collects what has not said which coin it runs.
    pub identity: Option<Identity>,
    pub chains: Vec<ChainGroup>,
}

impl CoinGroup {
    pub fn count(&self) -> usize {
        self.chains.iter().map(|chain| chain.entries.len()).sum()
    }
}

/// Tips compete at one height. A node a block behind is only behind.
fn competing_tips(entries: &[Entry]) -> usize {
    let mut by_height: BTreeMap<u64, BTreeSet<Hash>> = BTreeMap::new();
    for entry in entries {
        if let (Some(height), Some(tip)) = (entry.height, entry.tip) {
            by_height.entry(height).or_default().insert(tip);
        }
    }
    by_height.values().map(BTreeSet::len).max().unwrap_or(0)
}

fn chain_of(coin: Option<CoinConfig>, entries: Vec<Entry>, split: bool, yours: bool) -> ChainGroup {
    let tips = competing_tips(&entries);
    let relation = match (&coin, yours, split) {
        (None, _, _) => Relation::Unknown,
        (_, true, _) => Relation::Yours,
        (_, false, true) => Relation::Split,
        (_, false, false) => Relation::Network,
    };
    ChainGroup {
        rules: coin.as_ref().map(|coin| coin.rules_at(u64::MAX).clone()),
        coin,
        relation,
        entries,
        tips,
    }
}

/// One chain has one set of rules at every height. Rules that change later differ from the start
/// as far as a chain's future goes, so forks count too.
fn same_rules(one: &CoinConfig, other: &CoinConfig) -> bool {
    one.rules == other.rules && one.forks == other.forks
}

/// `mine` is the coin this app runs. Its coin comes first, then larger groups, then by name, so a
/// list does not jump around between looks.
pub fn group(entries: &[Entry], mine: Option<&Identity>) -> Vec<CoinGroup> {
    let mut coins: Vec<CoinGroup> = Vec::new();
    let mut unknown = Vec::new();
    for entry in entries {
        let Some(identity) = &entry.identity else {
            unknown.push(entry.clone());
            continue;
        };
        let at = coins.iter().position(|coin| {
            coin.identity
                .as_ref()
                .is_some_and(|known| known.genesis == identity.genesis)
        });
        let coin = match at {
            Some(at) => &mut coins[at],
            None => {
                coins.push(CoinGroup {
                    identity: Some(identity.clone()),
                    chains: Vec::new(),
                });
                let last = coins.len() - 1;
                &mut coins[last]
            }
        };
        let same = |chain: &&mut ChainGroup| {
            chain.coin.as_ref().is_some_and(|known| same_rules(known, &identity.coin))
        };
        match coin.chains.iter_mut().find(same) {
            Some(chain) => chain.entries.push(entry.clone()),
            None => {
                let first = vec![entry.clone()];
                coin.chains.push(chain_of(Some(identity.coin.clone()), first, false, false));
            }
        }
    }
    for coin in &mut coins {
        let genesis = coin.identity.as_ref().map(|identity| identity.genesis);
        let split = coin.chains.len() > 1;
        let mine_here = mine.filter(|identity| Some(identity.genesis) == genesis);
        let chains = std::mem::take(&mut coin.chains);
        coin.chains = chains
            .into_iter()
            .map(|chain| {
                let yours = mine_here.is_some_and(|identity| {
                    chain.coin.as_ref().is_some_and(|coin| same_rules(coin, &identity.coin))
                });
                // A chain beside yours is a split even when only it was found.
                let split = split || (mine_here.is_some() && !yours);
                chain_of(chain.coin, chain.entries, split, yours)
            })
            .collect();
        coin.chains.sort_by_key(|chain| {
            (
                chain.relation != Relation::Yours,
                std::cmp::Reverse(chain.entries.len()),
            )
        });
        for chain in &mut coin.chains {
            chain.entries.sort_by_key(sort_key);
        }
    }
    let is_mine = |coin: &CoinGroup| {
        coin.chains
            .iter()
            .any(|chain| chain.relation == Relation::Yours)
    };
    coins.sort_by_key(|coin| {
        let name = coin.identity.as_ref().map(|found| found.coin.name.clone());
        (!is_mine(coin), std::cmp::Reverse(coin.count()), name)
    });
    if !unknown.is_empty() {
        unknown.sort_by_key(sort_key);
        coins.push(CoinGroup {
            identity: None,
            chains: vec![chain_of(None, unknown, false, false)],
        });
    }
    coins
}

/// This app first, then near to far.
fn sort_key(entry: &Entry) -> (u8, String) {
    let order = match entry.reach {
        Reach::ThisApp => 0,
        Reach::ThisComputer => 1,
        Reach::Usb => 2,
        Reach::Radio(_) | Reach::Link(..) => 3,
        Reach::Lan => 4,
        Reach::Internet => 5,
    };
    (order, entry.name.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coin(name: &str) -> CoinConfig {
        CoinConfig {
            name: name.into(),
            ..CoinConfig::olc()
        }
    }

    fn entry(name: &str, coin: Option<CoinConfig>, tip: u8) -> Entry {
        Entry {
            reach: Reach::Lan,
            address: None,
            via: None,
            device: "computer".into(),
            name: name.into(),
            identity: coin.map(Identity::of),
            tip: Some([tip; 32]),
            height: Some(1),
            work_done: Some(1),
            build: "x".into(),
            work: Work::Idle,
            keeps: Keeps::Unsaid,
            age_secs: None,
        }
    }

    fn other_rules() -> CoinConfig {
        let mut coin = coin("One");
        coin.rules.block_time_ms = 1;
        coin
    }

    #[test]
    fn a_different_build_is_only_information() {
        let mut older = entry("a", Some(coin("One")), 1);
        older.build = "b12".into();
        let mut newer = entry("b", Some(coin("One")), 1);
        newer.build = "b13-test".into();
        let mine = Identity::of(coin("One"));
        let groups = group(&[older, newer], Some(&mine));
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].chains.len(), 1);
        let chain = &groups[0].chains[0];
        assert_eq!(chain.relation, Relation::Yours);
        let builds: Vec<&str> = chain.entries.iter().map(|entry| entry.build.as_str()).collect();
        assert_eq!(builds.len(), 2);
        assert!(builds.contains(&"b12") && builds.contains(&"b13-test"));
    }

    #[test]
    fn a_different_genesis_is_a_different_coin() {
        let found = [
            entry("a", Some(coin("One")), 1),
            entry("b", Some(coin("Two")), 1),
        ];
        let groups = group(&found, None);
        assert_eq!(groups.len(), 2);
        assert!(groups.iter().all(|coin| coin.chains.len() == 1));
        assert!(groups
            .iter()
            .all(|coin| coin.chains[0].relation == Relation::Network));
    }

    #[test]
    fn the_same_genesis_with_other_rules_is_a_split_chain() {
        let found = [
            entry("a", Some(coin("One")), 1),
            entry("b", Some(other_rules()), 2),
        ];
        let groups = group(&found, None);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].chains.len(), 2);
        assert!(groups[0]
            .chains
            .iter()
            .all(|chain| chain.relation == Relation::Split));
    }

    #[test]
    fn both_the_same_is_one_network_even_when_tips_compete() {
        let found = [
            entry("a", Some(coin("One")), 1),
            entry("b", Some(coin("One")), 2),
        ];
        let groups = group(&found, None);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].chains.len(), 1);
        assert_eq!(groups[0].chains[0].entries.len(), 2);
        assert_eq!(groups[0].chains[0].tips, 2);
    }

    #[test]
    fn a_node_a_block_behind_does_not_compete() {
        let ahead = entry("a", Some(coin("One")), 1);
        let behind = Entry {
            height: ahead.height.map(|height| height - 1),
            ..entry("b", Some(coin("One")), 2)
        };
        assert_eq!(group(&[ahead, behind], None)[0].chains[0].tips, 1);
    }

    #[test]
    fn your_chain_comes_first_and_a_lone_chain_beside_it_is_a_split() {
        let found = [
            entry("far", Some(coin("Two")), 1),
            entry("also far", Some(coin("Two")), 1),
            entry("b", Some(other_rules()), 2),
        ];
        let mine = Identity::of(coin("One"));
        let groups = group(&found, Some(&mine));
        assert_eq!(groups.len(), 2);
        // Only the split is on your coin, and it is not yours.
        let one = groups
            .iter()
            .find(|found| found.identity.as_ref().unwrap().coin.name == "One")
            .unwrap();
        assert_eq!(one.chains[0].relation, Relation::Split);
        let found = [entry("me", Some(coin("One")), 1), found[0].clone()];
        let groups = group(&found, Some(&mine));
        assert_eq!(groups[0].chains[0].relation, Relation::Yours);
        assert_eq!(groups[0].identity.as_ref().unwrap().coin.name, "One");
    }

    #[test]
    fn what_has_not_said_its_coin_stays_listed_at_the_end() {
        let found = [entry("old board", None, 1), entry("a", Some(coin("One")), 1)];
        let groups = group(&found, None);
        assert_eq!(groups.len(), 2);
        assert!(groups[1].identity.is_none());
        assert_eq!(groups[1].chains[0].relation, Relation::Unknown);
    }
}
