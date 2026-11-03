//! English words for mining on this computer and the settings around it.
pub const WORDS: &[(&str, &str)] = &[
    ("settings.speed_quarter", "Quarter"),
    ("settings.device_about", "Picks how many miners start on this computer. Your other settings stay."),
    ("storage.merge_about", "When 16 small coins pile up, joins 8 into one. Payments then need fewer coins."),
    ("local.preset_computer", "Computer sized"),
];

pub const EXPERT: &[(&str, &str)] = &[
    ("settings.speed_about", "Full rate here is {} on {} threads."),
    ("pro.cap_about", "0 means no cap. Applies when you press Enter or leave the field."),
    ("pro.threads", "Mining threads"),
    ("pro.threads_about", "This computer has {} cores. All threads share the cap."),
    ("storage.merge_about", "Spends 8 small UTXOs into one when 16 pile up. Off by default, as in Bitcoin Core."),
    ("settings.device_about", "Sets how many local nodes start. Caps and switches stay as set."),
];
