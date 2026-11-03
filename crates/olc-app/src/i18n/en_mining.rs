//! English words for mining on this computer and the settings around it.
pub const WORDS: &[(&str, &str)] = &[
    ("storage.merge_about", "When 16 small coins pile up, joins 8 into one. Payments then need fewer coins."),
    ("local.preset_computer", "Computer sized"),
];

pub const EXPERT: &[(&str, &str)] = &[
    ("pro.threads", "Mining threads"),
    ("pro.threads_about", "This computer has {} cores. All threads share the cap."),
    ("storage.merge_about", "Spends 8 small UTXOs into one when 16 pile up. Off by default, as in Bitcoin Core."),
];
