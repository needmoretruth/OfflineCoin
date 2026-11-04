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
    ("net.now", "Whole network now"),
    ("net.average", "Last {} blocks"),
    ("net.share", "This computer's share"),
    ("net.about", "Worked out from how fast the last blocks came, the way Bitcoin does it."),
];

pub const EXPERT: &[(&str, &str)] = &[
    ("budget.title", "Hashrate limit for this computer"),
    ("budget.about", "One limit for this app and every added miner: each running miner gets an equal part, or its own hash cap when that is smaller."),
    ("budget.equal_part", "Equal part of the limit"),
    ("budget.own_about", "0 takes an equal part. The sum of all miners stays under this computer's limit."),
    ("net.now", "Network hashrate"),
    ("net.average", "Average, last {} blocks"),
    ("net.about", "Work of the last 20 blocks over their time span, as getnetworkhashps reads it."),
];
