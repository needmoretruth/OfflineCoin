//! Beta scripts as text words, so a person writes `HASH 0x12ab EQUAL` and never bytes. Reading a
//! script back gives the same words, and a block explorer shows them.
use crate::beta::{op, signature_checks};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt::Write;
use olc_core::Address;
use olc_crypto::address_of;

/// One step of a script as text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Word {
    Op(u8),
    Data(Vec<u8>),
    /// A signature by the key of this address, which a wallet fills in. Only an unlock has these.
    SignBy(String),
    /// The public key of this address, which a wallet fills in the same way.
    KeyOf(String),
}

/// What is wrong with a script, for a person to fix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WordError {
    Unknown(String),
    TooLong(String),
    OpenQuote,
    OpenBranch,
    TooManyOps(usize),
    TooManyBytes(usize),
    TooManyChecks(usize),
    Empty,
}

impl core::fmt::Display for WordError {
    fn fmt(&self, out: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            Self::Unknown(word) => write!(out, "No word called {word}"),
            Self::TooLong(word) => write!(out, "{word} is too long for one word"),
            Self::OpenQuote => write!(out, "A quote is not closed"),
            Self::OpenBranch => write!(out, "IF, ELSE and ENDIF do not match"),
            Self::TooManyOps(count) => write!(out, "{count} words, and {MOST_OPS} is the most"),
            Self::TooManyBytes(count) => write!(out, "{count} bytes, and {MOST_LOCK_BYTES} is the most"),
            Self::TooManyChecks(count) => {
                write!(out, "{count} signature checks, and {MOST_SIGNATURE_CHECKS} is the most")
            }
            Self::Empty => write!(out, "No words"),
        }
    }
}

/// What a script costs to run. All nodes stop at the same limits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cost {
    pub bytes: usize,
    pub ops: usize,
    pub signature_checks: usize,
}

/// The most signature checks a node takes in one transaction. This is a node's policy, as Bitcoin's
/// standard transactions are, and a block holding more is still valid. A board at 80 MHz took 2.5 s
/// for a block of 8 merged coins, so a check costs it about 310 ms at most: 32 checks hold it for
/// about 10 s, a third of the usual block time, and 32 plain inputs fill a usual block of 4,096 bytes.
pub const MOST_SIGNATURE_CHECKS: usize = 32;
/// One lock with its unlock must fit in this many steps, as [`crate::beta`] counts them.
pub const MOST_OPS: usize = 200;
/// A lock travels inside a transaction output, and a board has to keep that coin.
pub const MOST_LOCK_BYTES: usize = 1_000;
const MOST_PUSH: usize = op::PUSH_MAX as usize;

const NAMES: &[(&str, u8)] = &[
    ("IF", op::IF),
    ("ELSE", op::ELSE),
    ("ENDIF", op::END_IF),
    ("VERIFY", op::VERIFY),
    ("RETURN", op::RETURN),
    ("DROP", op::DROP),
    ("DUP", op::DUP),
    ("SWAP", op::SWAP),
    ("EQUAL", op::EQUAL),
    ("EQUALVERIFY", op::EQUAL_VERIFY),
    ("ADD", op::ADD),
    ("HASH", op::HASH),
    ("CHECKSIG", op::CHECK_SIG),
    ("CHECKMULTISIG", op::CHECK_MULTI_SIG),
    ("CHECKHEIGHT", op::CHECK_HEIGHT),
];

fn hex_value(byte: u8) -> Option<u8> {
    char::from(byte).to_digit(16).and_then(|digit| u8::try_from(digit).ok())
}

fn from_hex(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    let pair = |pair: &[u8]| Some(hex_value(pair[0])? * 16 + hex_value(pair[1])?);
    bytes.chunks(2).map(pair).collect()
}

fn number_bytes(value: u64) -> Vec<u8> {
    let bytes = value.to_le_bytes();
    let length = bytes.iter().rposition(|byte| *byte != 0).map_or(0, |at| at + 1);
    bytes[..length].to_vec()
}

fn data_word(bytes: Vec<u8>, shown: &str) -> Result<Word, WordError> {
    match bytes.len() > MOST_PUSH {
        true => Err(WordError::TooLong(shown.to_string())),
        false => Ok(Word::Data(bytes)),
    }
}

fn one_word(text: &str) -> Result<Word, WordError> {
    let upper = text.to_ascii_uppercase();
    if let Some((_, code)) = NAMES.iter().find(|(name, _)| *name == upper) {
        return Ok(Word::Op(*code));
    }
    if let Some(hex) = text.strip_prefix("0x") {
        let bytes = from_hex(hex).ok_or_else(|| WordError::Unknown(text.to_string()))?;
        return data_word(bytes, text);
    }
    if let Some(address) = text.strip_prefix("sig:") {
        return Ok(Word::SignBy(address.to_string()));
    }
    if let Some(address) = text.strip_prefix("key:") {
        return Ok(Word::KeyOf(address.to_string()));
    }
    match text.parse::<u64>() {
        Ok(0) => Ok(Word::Op(op::ZERO)),
        Ok(1) => Ok(Word::Op(op::ONE)),
        Ok(value) => Ok(Word::Data(number_bytes(value))),
        Err(_) => Err(WordError::Unknown(text.to_string())),
    }
}

/// Splits text into words. `"..."` is one word of text, with `\"` and `\\` inside.
fn split_words(text: &str) -> Result<Vec<Word>, WordError> {
    let mut words = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(next) = chars.peek().copied() {
        if next.is_whitespace() {
            chars.next();
            continue;
        }
        if next == '"' {
            chars.next();
            let mut inside = String::new();
            loop {
                match chars.next().ok_or(WordError::OpenQuote)? {
                    '"' => break,
                    '\\' => inside.push(chars.next().ok_or(WordError::OpenQuote)?),
                    other => inside.push(other),
                }
            }
            words.push(data_word(inside.clone().into_bytes(), &inside)?);
            continue;
        }
        let mut word = String::new();
        while let Some(next) = chars.peek().copied().filter(|next| !next.is_whitespace()) {
            word.push(next);
            chars.next();
        }
        words.push(one_word(&word)?);
    }
    Ok(words)
}

fn encode(words: &[Word]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for word in words {
        match word {
            Word::Op(code) => bytes.push(*code),
            Word::Data(data) if data.is_empty() => bytes.push(op::ZERO),
            Word::Data(data) => {
                bytes.push(data.len() as u8);
                bytes.extend_from_slice(data);
            }
            Word::SignBy(_) | Word::KeyOf(_) => {}
        }
    }
    bytes
}

fn branches_close(words: &[Word]) -> bool {
    let mut open = 0usize;
    for word in words {
        match word {
            Word::Op(op::IF) => open += 1,
            Word::Op(op::ELSE) if open == 0 => return false,
            Word::Op(op::END_IF) => match open.checked_sub(1) {
                Some(left) => open = left,
                None => return false,
            },
            _ => {}
        }
    }
    open == 0
}

/// Text to words, for a lock or an unlock.
pub fn parse_words(text: &str) -> Result<Vec<Word>, WordError> {
    let words = split_words(text)?;
    if words.is_empty() {
        return Err(WordError::Empty);
    }
    if !branches_close(&words) {
        return Err(WordError::OpenBranch);
    }
    Ok(words)
}

/// Words to the bytes of a script, with what it costs. A lock has no `sig:` words.
pub fn assemble(words: &[Word]) -> Result<(Vec<u8>, Cost), WordError> {
    for word in words {
        match word {
            Word::SignBy(address) => return Err(WordError::Unknown(format!("sig:{address}"))),
            Word::KeyOf(address) => return Err(WordError::Unknown(format!("key:{address}"))),
            _ => {}
        }
    }
    let bytes = encode(words);
    let cost = Cost { bytes: bytes.len(), ops: words.len(), signature_checks: signature_checks(&bytes) };
    if cost.ops > MOST_OPS {
        return Err(WordError::TooManyOps(cost.ops));
    }
    if bytes.len() > MOST_LOCK_BYTES {
        return Err(WordError::TooManyBytes(bytes.len()));
    }
    // Coins under such a lock could never be spent, because no node would pass the spend on.
    if cost.signature_checks > MOST_SIGNATURE_CHECKS {
        return Err(WordError::TooManyChecks(cost.signature_checks));
    }
    Ok((bytes, cost))
}

/// Text to the bytes of a lock.
pub fn lock_from_text(text: &str) -> Result<(Vec<u8>, Cost), WordError> {
    assemble(&parse_words(text)?)
}

/// The number these bytes are, when `number_bytes` gives the same bytes back.
fn number_of(data: &[u8]) -> Option<u64> {
    let mut wide = [0; 8];
    wide.get_mut(..data.len())?.copy_from_slice(data);
    let number = u64::from_le_bytes(wide);
    (number_bytes(number) == data).then_some(number)
}

fn show_data(data: &[u8], out: &mut String) {
    let printable = data.iter().all(|byte| byte.is_ascii_graphic() || *byte == b' ');
    if printable && data.len() > 1 && core::str::from_utf8(data).is_ok() && data.len() < 20 {
        out.push('"');
        for byte in data {
            if *byte == b'"' || *byte == b'\\' {
                out.push('\\');
            }
            out.push(char::from(*byte));
        }
        out.push('"');
    } else {
        out.push_str("0x");
        for byte in data {
            let _ = write!(out, "{byte:02x}");
        }
    }
}

fn name_of(code: u8) -> Option<&'static str> {
    NAMES.iter().find(|(_, found)| *found == code).map(|(name, _)| *name)
}

/// The words of a script. A byte that has no word shows as `?xx`, so a person sees it is not a
/// script that can run, and the rest still reads.
pub fn words_of(script: &[u8]) -> String {
    let mut out = String::new();
    let mut at = 0;
    while at < script.len() {
        if !out.is_empty() {
            out.push(' ');
        }
        let code = script[at];
        at += 1;
        if (1..=op::PUSH_MAX).contains(&code) {
            let end = at + usize::from(code);
            let counts = matches!(script.get(end), Some(&op::CHECK_HEIGHT | &op::ADD));
            match script.get(at..end) {
                Some(data) if counts || data.len() == 1 => match number_of(data) {
                    Some(number) => {
                        let _ = write!(out, "{number}");
                    }
                    None => show_data(data, &mut out),
                },
                Some(data) => show_data(data, &mut out),
                None => {
                    out.push_str("?cut");
                    break;
                }
            }
            at = end;
            continue;
        }
        match (code, name_of(code)) {
            (op::ZERO, _) => out.push('0'),
            (op::ONE, _) => out.push('1'),
            (_, Some(name)) => out.push_str(name),
            (_, None) => {
                let _ = write!(out, "?{code:02x}");
            }
        }
    }
    out
}

/// Words as text that reads back as the same words, with signatures still to come.
pub fn text_of_words(words: &[Word]) -> String {
    let mut out = String::new();
    for word in words {
        if !out.is_empty() {
            out.push(' ');
        }
        match word {
            Word::Op(code) => out.push_str(&words_of(&[*code])),
            Word::Data(data) if data.is_empty() => out.push('0'),
            Word::Data(data) => show_hex(data, &mut out),
            Word::SignBy(address) => {
                let _ = write!(out, "sig:{address}");
            }
            Word::KeyOf(address) => {
                let _ = write!(out, "key:{address}");
            }
        }
    }
    out
}

/// A lock only the holder of the words whose hash this is can open.
pub fn hash_lock(hash: &[u8]) -> String {
    let mut out = String::from("HASH ");
    show_hex(hash, &mut out);
    out.push_str(" EQUAL");
    out
}

/// A lock that needs both keys to sign.
pub fn two_of_two(first: &[u8], second: &[u8]) -> String {
    let mut out = String::from("2 ");
    show_hex(first, &mut out);
    out.push(' ');
    show_hex(second, &mut out);
    out.push_str(" 2 CHECKMULTISIG");
    out
}

/// A lock this address opens from `height` on.
pub fn time_lock(height: u64, address: &Address) -> String {
    let mut out = format!("{height} CHECKHEIGHT DROP DUP HASH ");
    show_hex(address, &mut out);
    out.push_str(" EQUALVERIFY CHECKSIG");
    out
}

/// A public key typed as hex.
pub fn public_key_from_text(text: &str) -> Option<[u8; 33]> {
    let text = text.trim();
    from_hex(text.strip_prefix("0x").unwrap_or(text))?.try_into().ok()
}

fn signer(key: &[u8]) -> Option<String> {
    let mut out = String::new();
    for byte in address_of(key.try_into().ok()?) {
        let _ = write!(out, "{byte:02x}");
    }
    Some(out)
}

/// The unlock words a lock of a preset's shape takes, with the signatures still to come. A hash
/// lock's unlock is a person's own secret, so it has none to offer.
pub fn unlock_for(lock: &[u8]) -> String {
    use Word::{Data, Op};
    let words = parse_words(&words_of(lock)).unwrap_or_default();
    match words.as_slice() {
        [Data(_), Data(first), Data(second), Data(_), Op(op::CHECK_MULTI_SIG)] => {
            let first = signer(first).zip(signer(second));
            first.map_or_else(String::new, |(a, b)| format!("sig:{a} sig:{b}"))
        }
        [Data(_), Op(op::CHECK_HEIGHT), Op(op::DROP), Op(op::DUP), Op(op::HASH), Data(address), ..] => {
            let mut out = String::new();
            for byte in address {
                let _ = write!(out, "{byte:02x}");
            }
            format!("sig:{out} key:{out}")
        }
        _ => String::new(),
    }
}

fn show_hex(bytes: &[u8], out: &mut String) {
    out.push_str("0x");
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::beta::{unlock_address, Beta};
    use olc_core::{LockLanguage, SpendContext};
    use olc_crypto::{address_of, hash, public_key, sign};

    fn context(height: u64) -> SpendContext {
        SpendContext { signature_hash: hash(b"spend"), height }
    }

    fn unlocks(lock: &str, unlock: &[u8], height: u64) -> bool {
        let (lock, _) = lock_from_text(lock).unwrap();
        Beta.check(&lock, unlock, &context(height))
    }

    fn signed(seed: u8) -> ([u8; 33], [u8; 64]) {
        let secret = [seed; 32];
        (public_key(&secret).unwrap(), sign(&secret, &hash(b"spend")).unwrap())
    }

    fn push(script: &mut Vec<u8>, data: &[u8]) {
        script.push(data.len() as u8);
        script.extend_from_slice(data);
    }

    #[test]
    fn the_hash_lock_opens_for_the_secret_alone() {
        let lock = hash_lock(&hash(b"open sesame")[..20]);
        let (right, _) = assemble(&parse_words("\"open sesame\"").unwrap()).unwrap();
        let (wrong, _) = assemble(&parse_words("\"open barley\"").unwrap()).unwrap();
        assert!(unlocks(&lock, &right, 0));
        assert!(!unlocks(&lock, &wrong, 0));
    }

    #[test]
    fn two_of_two_needs_both_signatures_in_order() {
        let (first, first_signature) = signed(1);
        let (second, second_signature) = signed(2);
        let lock = two_of_two(&first, &second);
        let mut both = Vec::new();
        push(&mut both, &first_signature);
        push(&mut both, &second_signature);
        assert!(unlocks(&lock, &both, 0));
        let mut one = Vec::new();
        push(&mut one, &second_signature);
        assert!(!unlocks(&lock, &one, 0));
        let mut twice = Vec::new();
        push(&mut twice, &first_signature);
        push(&mut twice, &first_signature);
        assert!(!unlocks(&lock, &twice, 0));
    }

    #[test]
    fn the_time_lock_opens_at_its_height() {
        let (key, signature) = signed(3);
        let lock = time_lock(500, &address_of(&key));
        let unlock = unlock_address(&key, &signature);
        assert!(!unlocks(&lock, &unlock, 499));
        assert!(unlocks(&lock, &unlock, 500));
    }

    #[test]
    fn words_read_back_as_they_were_written() {
        for text in [
            "DUP HASH 0x0102030405060708090a0b0c0d0e0f1011121314 EQUALVERIFY CHECKSIG",
            "1 IF \"yes\" ELSE 0 ENDIF DROP",
            "500 CHECKHEIGHT DROP 1",
            "SWAP ADD VERIFY RETURN",
        ] {
            let (bytes, _) = lock_from_text(text).unwrap();
            assert_eq!(words_of(&bytes), text);
        }
    }

    #[test]
    fn lower_case_words_are_the_same_words() {
        assert_eq!(lock_from_text("dup hash").unwrap(), lock_from_text("DUP HASH").unwrap());
    }

    #[test]
    fn a_bad_script_says_what_is_wrong() {
        let wrong = |text: &str| lock_from_text(text).unwrap_err();
        assert_eq!(wrong("DUP SPIN"), WordError::Unknown("SPIN".into()));
        assert_eq!(wrong("0xabc"), WordError::Unknown("0xabc".into()));
        assert_eq!(wrong("\"open"), WordError::OpenQuote);
        assert_eq!(wrong("IF 1"), WordError::OpenBranch);
        assert_eq!(wrong("ENDIF"), WordError::OpenBranch);
        assert_eq!(wrong("  "), WordError::Empty);
        assert_eq!(wrong(&format!("0x{}", "ab".repeat(76))), WordError::TooLong(format!("0x{}", "ab".repeat(76))));
        assert_eq!(wrong(&"DUP ".repeat(201)), WordError::TooManyOps(201));
        assert_eq!(wrong("sig:abc"), WordError::Unknown("sig:abc".into()));
    }

    #[test]
    fn a_script_says_what_it_costs() {
        let (bytes, cost) = lock_from_text("DUP HASH 0x0102 EQUALVERIFY CHECKSIG").unwrap();
        assert_eq!(cost, Cost { bytes: bytes.len(), ops: 5, signature_checks: 1 });
        assert_eq!(bytes.len(), 7);
    }

    #[test]
    fn bytes_that_are_no_script_still_read() {
        assert_eq!(words_of(&[0xff, op::DUP]), "?ff DUP");
        assert_eq!(words_of(&[5, 1]), "?cut");
        assert_eq!(words_of(&[]), "");
    }

    #[test]
    fn an_unlock_keeps_its_signature_place_for_the_wallet() {
        let words = parse_words("sig:abc key:abc \"text\"").unwrap();
        assert_eq!(words[0], Word::SignBy("abc".into()));
        assert_eq!(words[1], Word::KeyOf("abc".into()));
        assert_eq!(words[2], Word::Data(b"text".to_vec()));
        assert_eq!(parse_words(&text_of_words(&words)).unwrap(), words);
    }
}
