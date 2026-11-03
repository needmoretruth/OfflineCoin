//! Why a node refuses a payment, as the key of a notice. The app says each in the reader's
//! language and the command line in English.
use olc_core::ChainError;

/// Every reason a chain gives. A reason added to [`ChainError`] stops [`refusal_key`] from
/// building until it has words, and belongs here as well so the tests see it.
pub const ALL: [ChainError; 20] = [
    ChainError::AlreadyKnown,
    ChainError::BadSeal,
    ChainError::BadDifficulty,
    ChainError::BadMerkleRoot,
    ChainError::BadTime,
    ChainError::TooLarge,
    ChainError::NoCoinbase,
    ChainError::BadCoinbase,
    ChainError::MissingInput,
    ChainError::DuplicateInput,
    ChainError::ImmatureCoinbase,
    ChainError::BadLock,
    ChainError::UnknownLanguage,
    ChainError::Overspend,
    ChainError::FeeTooLow,
    ChainError::ReorgTooDeep,
    ChainError::StoreFailed,
    ChainError::NotYetAllowed,
    ChainError::NotFinal,
    ChainError::NeedsNewVersion,
];

/// The notice that says why the chain refuses a payment.
pub fn refusal_key(why: ChainError) -> &'static str {
    match why {
        ChainError::NotFinal => "notice.refused_not_final",
        ChainError::NeedsNewVersion => "notice.refused_needs_new_version",
        ChainError::NotYetAllowed => "notice.refused_not_yet_allowed",
        ChainError::MissingInput => "notice.refused_missing_input",
        ChainError::DuplicateInput => "notice.refused_duplicate_input",
        ChainError::BadLock => "notice.refused_bad_lock",
        ChainError::Overspend => "notice.refused_overspend",
        ChainError::FeeTooLow => "notice.refused_fee_too_low",
        ChainError::ImmatureCoinbase => "notice.refused_immature_coinbase",
        ChainError::TooLarge => "notice.refused_too_large",
        ChainError::UnknownLanguage => "notice.refused_unknown_language",
        ChainError::AlreadyKnown => "notice.refused_already_known",
        // A block's faults, which a payment alone never has, and a store that failed.
        ChainError::BadSeal
        | ChainError::BadDifficulty
        | ChainError::BadMerkleRoot
        | ChainError::BadTime
        | ChainError::NoCoinbase
        | ChainError::BadCoinbase
        | ChainError::ReorgTooDeep
        | ChainError::StoreFailed => "notice.refused_rules",
    }
}

/// The refusal `key` names, when it names one.
pub fn known(key: &str) -> Option<&'static str> {
    ALL.into_iter().map(refusal_key).find(|known| *known == key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_reason_is_listed_once_and_has_a_key_of_its_own_kind() {
        for (at, why) in ALL.iter().enumerate() {
            assert!(!ALL[..at].contains(why), "{why:?} is listed twice");
            assert_eq!(known(refusal_key(*why)), Some(refusal_key(*why)));
        }
        assert_eq!(known("notice.saved"), None);
    }
}
