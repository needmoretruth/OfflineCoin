//! Beta and gamma scripts as text words, so a person writes `HASH 0x12ab EQUAL` and never bytes.
//! Reading a script back gives the same words, and a block explorer shows them. The functions
//! without `_in` read beta, as they always did.
use crate::beta::{op, signature_checks};
use crate::{gamma, gamma_words, BETA, GAMMA};
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
/// standard transactions are, and a block holding more is still valid. A board at 80 MHz measured
/// 115 ms for its slowest check, and it answers nobody while it checks. A neighbour counts as gone
/// after 3,500 ms of silence with hellos a second apart, which leaves 2,500 ms: 20 checks take
/// 2,300 ms and leave the rest for reading the transaction. So a stranger's payment cannot cost
/// a board its links. It does not bound a block, which takes real work to make.
pub const MOST_SIGNATURE_CHECKS: usize = 20;
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

/// Whose words: beta's, the 2009 set, or gamma's, which are today's Bitcoin's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialect {
    Beta,
    Gamma,
}

impl Dialect {
    /// The words of a lock language, for the two that have words.
    pub fn of(language: u8) -> Option<Self> {
        match language {
            BETA => Some(Self::Beta),
            GAMMA => Some(Self::Gamma),
            _ => None,
        }
    }

    pub fn language(self) -> u8 {
        match self {
            Self::Beta => BETA,
            Self::Gamma => GAMMA,
        }
    }

    fn names(self) -> &'static [(&'static str, u8)] {
        match self {
            Self::Beta => NAMES,
            Self::Gamma => gamma_words::NAMES,
        }
    }

    fn checks(self, script: &[u8]) -> usize {
        match self {
            Self::Beta => signature_checks(script),
            Self::Gamma => gamma::signature_checks(script),
        }
    }
}

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

fn one_word(dialect: Dialect, text: &str) -> Result<Word, WordError> {
    let upper = text.to_ascii_uppercase();
    if let Some((_, code)) = dialect.names().iter().find(|(name, _)| *name == upper) {
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
    if dialect == Dialect::Gamma {
        return gamma_words::number_word(text).ok_or_else(|| WordError::Unknown(text.to_string()));
    }
    match text.parse::<u64>() {
        Ok(0) => Ok(Word::Op(op::ZERO)),
        Ok(1) => Ok(Word::Op(op::ONE)),
        Ok(value) => Ok(Word::Data(number_bytes(value))),
        Err(_) => Err(WordError::Unknown(text.to_string())),
    }
}

/// Splits text into words. `"..."` is one word of text, with `\"` and `\\` inside.
fn split_words(dialect: Dialect, text: &str) -> Result<Vec<Word>, WordError> {
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
        words.push(one_word(dialect, &word)?);
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
            Word::Op(op::IF | gamma::op::NOT_IF) => open += 1,
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
    parse_words_in(Dialect::Beta, text)
}

/// Text to words in `dialect`.
pub fn parse_words_in(dialect: Dialect, text: &str) -> Result<Vec<Word>, WordError> {
    let words = split_words(dialect, text)?;
    if words.is_empty() {
        return Err(WordError::Empty);
    }
    if !branches_close(&words) {
        return Err(WordError::OpenBranch);
    }
    Ok(words)
}

/// The number a word stands for in `dialect`, if it is one.
pub fn number_in(dialect: Dialect, word: &Word) -> Option<i64> {
    match (dialect, word) {
        (Dialect::Gamma, Word::Op(code)) => gamma_words::small_number(*code),
        (Dialect::Gamma, Word::Data(data)) => gamma_words::number_of(data),
        (Dialect::Beta, Word::Op(op::ZERO)) => Some(0),
        (Dialect::Beta, Word::Op(op::ONE)) => Some(1),
        (Dialect::Beta, Word::Data(data)) => number_of(data).and_then(|number| i64::try_from(number).ok()),
        _ => None,
    }
}

/// Words to the bytes of a script, with what it costs. A lock has no `sig:` words.
pub fn assemble(words: &[Word]) -> Result<(Vec<u8>, Cost), WordError> {
    assemble_in(Dialect::Beta, words)
}

/// Words to the bytes of a script in `dialect`, with what it costs.
pub fn assemble_in(dialect: Dialect, words: &[Word]) -> Result<(Vec<u8>, Cost), WordError> {
    for word in words {
        match word {
            Word::SignBy(address) => return Err(WordError::Unknown(format!("sig:{address}"))),
            Word::KeyOf(address) => return Err(WordError::Unknown(format!("key:{address}"))),
            _ => {}
        }
    }
    let bytes = encode(words);
    let cost = Cost { bytes: bytes.len(), ops: words.len(), signature_checks: dialect.checks(&bytes) };
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
    lock_from_text_in(Dialect::Beta, text)
}

/// Text to the bytes of a lock in `dialect`.
pub fn lock_from_text_in(dialect: Dialect, text: &str) -> Result<(Vec<u8>, Cost), WordError> {
    assemble_in(dialect, &parse_words_in(dialect, text)?)
}

/// The number these bytes are, when `number_bytes` gives the same bytes back.
fn number_of(data: &[u8]) -> Option<u64> {
    let mut wide = [0; 8];
    wide.get_mut(..data.len())?.copy_from_slice(data);
    let number = u64::from_le_bytes(wide);
    (number_bytes(number) == data).then_some(number)
}

pub(crate) fn show_data(data: &[u8], out: &mut String) {
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

fn name_of(dialect: Dialect, code: u8) -> Option<&'static str> {
    dialect.names().iter().find(|(_, found)| *found == code).map(|(name, _)| *name)
}

/// The words of a script. A byte that has no word shows as `?xx`, so a person sees it is not a
/// script that can run, and the rest still reads.
pub fn words_of(script: &[u8]) -> String {
    words_of_in(Dialect::Beta, script)
}

/// The words of a script in `dialect`.
pub fn words_of_in(dialect: Dialect, script: &[u8]) -> String {
    if dialect == Dialect::Gamma {
        return gamma_words::words_of(script);
    }
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
        match (code, name_of(Dialect::Beta, code)) {
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
    text_of_words_in(Dialect::Beta, words)
}

/// Words in `dialect` as text that reads back as the same words.
pub fn text_of_words_in(dialect: Dialect, words: &[Word]) -> String {
    let mut out = String::new();
    for word in words {
        if !out.is_empty() {
            out.push(' ');
        }
        match word {
            Word::Op(code) => out.push_str(&words_of_in(dialect, &[*code])),
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

/// A gamma lock this address opens once the coin has stood `blocks` blocks in the chain, with the
/// spending input's sequence at least that (BIP 68 and 112).
pub fn age_lock(blocks: u64, address: &Address) -> String {
    let mut out = format!("{blocks} CHECKSEQUENCEVERIFY DROP DUP HASH ");
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
    unlock_for_in(Dialect::Beta, lock)
}

/// [`unlock_for`] for a lock in `dialect`. Gamma writes small numbers as words of their own.
pub fn unlock_for_in(dialect: Dialect, lock: &[u8]) -> String {
    use Word::{Data, Op};
    let words = parse_words_in(dialect, &words_of_in(dialect, lock)).unwrap_or_default();
    let number = |word: &Word| matches!(word, Data(_)) || matches!(word, Op(code) if gamma_words::small_number(*code).is_some());
    let waits = |code: u8| code == op::CHECK_HEIGHT || (dialect == Dialect::Gamma && code == gamma::op::CHECK_SEQUENCE);
    match words.as_slice() {
        [count, Data(first), Data(second), total, Op(op::CHECK_MULTI_SIG)] if number(count) && number(total) => {
            let first = signer(first).zip(signer(second));
            first.map_or_else(String::new, |(a, b)| format!("sig:{a} sig:{b}"))
        }
        [height, Op(code), Op(op::DROP), Op(op::DUP), Op(op::HASH), Data(address), ..] if number(height) && waits(*code) => {
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
#[path = "words_tests.rs"]
mod tests;
