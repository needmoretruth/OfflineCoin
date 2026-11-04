//! English words for the one speed limit every miner on this computer shares, and for the whole
//! network as the last blocks show it.
pub const WORDS: &[(&str, &str)] = &[
    ("budget.title", "Mining speed on this computer"),
    ("budget.about", "This app and the miners you add share this limit. Higher finds blocks more often and keeps the computer busier."),
    ("budget.split", "This app {} · Added miners {}"),
    ("budget.next", "A new miner starts at {}."),
    ("budget.equal_part", "Equal part"),
    ("budget.own_about", "0 takes an equal part. No miner goes past this computer's limit."),
    ("chain.ceiling_limit", "Limit {}"),
    ("chain.ceiling_full", "Top speed {}"),
    ("chain.own_pool", "Your pool"),
    ("chain.pool", "Pool {}"),
    ("chain.pool_workers", "Working in it: {}"),
    ("net.title", "The whole network"),
    ("net.now", "Network now"),
    ("net.now_span", "Over the last {}"),
    ("net.average", "Last {} blocks"),
    ("net.average_late", "Follows a change of speed about {} late"),
    ("net.share", "Computer's share"),
    ("net.about", "Counted from the blocks of the last {}, {} of them. A change of speed shows in full after {}."),
    ("net.too_few", "The network speed shows once {} blocks are made. {} so far."),
];

pub const EXPERT: &[(&str, &str)] = &[
    ("budget.title", "Hashrate limit for this computer"),
    ("budget.about", "One limit for this app and every added miner: each running miner gets an equal part, or its own hash cap when that is smaller."),
    ("budget.equal_part", "Equal part of the limit"),
    ("budget.own_about", "0 takes an equal part. The sum of all miners stays under this computer's limit."),
    ("net.now", "Network hashrate"),
    ("net.average", "{}-block average"),
    ("net.average_late", "Lags a change of hashrate by about {}"),
    ("net.share", "Hashrate share"),
    ("net.about", "Work of the blocks in the last {} ({} blocks) over that time, as getnetworkhashps reads it. A change of hashrate shows in full after {}."),
    ("net.too_few", "Network hashrate is estimated from {} blocks on. {} so far."),
];
