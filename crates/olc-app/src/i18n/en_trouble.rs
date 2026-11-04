//! The Lab's bug exploiter, in plain words. For each trick one line says what it tried, and one
//! says which rule stopped it. Plain words only: a test bans the terms of the trade here.
pub const WORDS: &[(&str, &str)] = &[
    ("trouble.extra_reward", "Tried to pay itself a bigger mining reward than the rules allow."),
    ("trouble.extra_reward_rule", "A block may hand out only the set reward plus the fees it gathers."),
    ("trouble.double_spend", "Tried to spend the same coin twice in one payment."),
    ("trouble.double_spend_rule", "A coin can be spent only once."),
    ("trouble.spend_missing", "Tried to spend a coin that was never made."),
    ("trouble.spend_missing_rule", "You can spend only a coin that exists and has not been spent yet."),
    ("trouble.forged_signature", "Tried to spend someone else's coin with a made-up signature."),
    ("trouble.forged_signature_rule", "A coin moves only with a signature from the key that owns it."),
    ("trouble.immature_reward", "Tried to spend a mining reward before it is old enough to spend."),
    ("trouble.immature_reward_rule", "A mining reward has to age a set number of blocks before it can be spent."),
    ("trouble.future_block", "Sent a block stamped far ahead of the clock."),
    ("trouble.future_block_rule", "A block timed too far ahead of now is not taken."),
    ("trouble.too_little_work", "Sent a block with too little work behind it."),
    ("trouble.too_little_work_rule", "A block has to carry enough work to meet its target."),
    ("trouble.oversize_block", "Sent a block bigger than the size limit."),
    ("trouble.oversize_block_rule", "A block may not be larger than the size limit."),
];

pub const EXPERT: &[(&str, &str)] = &[
    ("trouble.extra_reward", "Inflated the coinbase output above the block subsidy plus fees."),
    ("trouble.extra_reward_rule", "Rejected (BadCoinbase): coinbase value exceeds subsidy plus collected fees."),
    ("trouble.double_spend", "Reused one outpoint as two inputs of a single transaction."),
    ("trouble.double_spend_rule", "Rejected (DuplicateInput): an outpoint may be spent once."),
    ("trouble.spend_missing", "Spent an outpoint absent from the UTXO set."),
    ("trouble.spend_missing_rule", "Rejected (MissingInput): the input is not in the UTXO set."),
    ("trouble.forged_signature", "Spent another key's UTXO with an unlock that does not satisfy its script."),
    ("trouble.forged_signature_rule", "Rejected (BadLock): the signature does not satisfy the output script."),
    ("trouble.immature_reward", "Spent a coinbase output before the coinbase maturity."),
    ("trouble.immature_reward_rule", "Rejected (ImmatureCoinbase): a coinbase matures a set number of blocks after it is mined."),
    ("trouble.future_block", "Block timestamp beyond the max-future-time window (time-too-new)."),
    ("trouble.future_block_rule", "Rejected (BadTime): the block time is past the node's future-time window."),
    ("trouble.too_little_work", "Header hash does not meet the target set by its difficulty."),
    ("trouble.too_little_work_rule", "Rejected (BadSeal): the proof of work does not meet the target."),
    ("trouble.oversize_block", "Serialized block exceeds the maximum block size."),
    ("trouble.oversize_block_rule", "Rejected (TooLarge): the block is over the maximum block size."),
];
