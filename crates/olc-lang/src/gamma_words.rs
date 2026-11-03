//! Gamma's words: Bitcoin's names, and numbers written as Bitcoin writes them.
use crate::gamma::op::*;
use crate::gamma::{number_bytes, read_number, read_push};
use crate::words::{show_data, Word};
use alloc::string::String;
use core::fmt::Write;

pub(crate) const NAMES: &[(&str, u8)] = &[
    ("NOP", NOP),
    ("IF", IF),
    ("NOTIF", NOT_IF),
    ("ELSE", ELSE),
    ("ENDIF", END_IF),
    ("VERIFY", VERIFY),
    ("RETURN", RETURN),
    ("TOALTSTACK", TO_ALT),
    ("FROMALTSTACK", FROM_ALT),
    ("2DROP", DROP_2),
    ("2DUP", DUP_2),
    ("3DUP", DUP_3),
    ("2OVER", OVER_2),
    ("2ROT", ROT_2),
    ("2SWAP", SWAP_2),
    ("IFDUP", IF_DUP),
    ("DEPTH", DEPTH),
    ("DROP", DROP),
    ("DUP", DUP),
    ("NIP", NIP),
    ("OVER", OVER),
    ("PICK", PICK),
    ("ROLL", ROLL),
    ("ROT", ROT),
    ("SWAP", SWAP),
    ("TUCK", TUCK),
    ("SIZE", SIZE),
    ("EQUAL", EQUAL),
    ("EQUALVERIFY", EQUAL_VERIFY),
    ("1ADD", ADD_1),
    ("1SUB", SUB_1),
    ("NEGATE", NEGATE),
    ("ABS", ABS),
    ("NOT", NOT),
    ("0NOTEQUAL", NOT_ZERO),
    ("ADD", ADD),
    ("SUB", SUB),
    ("BOOLAND", BOOL_AND),
    ("BOOLOR", BOOL_OR),
    ("NUMEQUAL", NUM_EQUAL),
    ("NUMEQUALVERIFY", NUM_EQUAL_VERIFY),
    ("NUMNOTEQUAL", NUM_NOT_EQUAL),
    ("LESSTHAN", LESS),
    ("GREATERTHAN", GREATER),
    ("LESSTHANOREQUAL", LESS_OR_EQUAL),
    ("GREATERTHANOREQUAL", GREATER_OR_EQUAL),
    ("MIN", MIN),
    ("MAX", MAX),
    ("WITHIN", WITHIN),
    ("RIPEMD160", RIPEMD160),
    ("SHA256", SHA256),
    ("HASH", HASH),
    ("HASH256", HASH256),
    ("HASH160", HASH160),
    ("CHECKSIG", CHECK_SIG),
    ("CHECKSIGVERIFY", CHECK_SIG_VERIFY),
    ("CHECKMULTISIG", CHECK_MULTI_SIG),
    ("CHECKMULTISIGVERIFY", CHECK_MULTI_SIG_VERIFY),
    ("CHECKHEIGHT", CHECK_HEIGHT),
    ("CHECKSEQUENCEVERIFY", CHECK_SEQUENCE),
];

/// The most a lock may ask CHECKHEIGHT or CHECKSEQUENCEVERIFY for: five bytes of script number.
const MOST_NUMBER: i64 = (1 << 39) - 1;

/// A number as Bitcoin writes it: 0, -1 and 1 to 16 as their own words, any other as data.
pub(crate) fn number_word(text: &str) -> Option<Word> {
    let value: i64 = text.parse().ok().filter(|value: &i64| value.abs() <= MOST_NUMBER)?;
    Some(match value {
        0 => Word::Op(ZERO),
        -1 => Word::Op(NEGATIVE_ONE),
        1..=16 => Word::Op(ONE + u8::try_from(value - 1).ok()?),
        _ => Word::Data(number_bytes(value)),
    })
}

/// The number these bytes are, when it is written the shortest way.
pub(crate) fn number_of(data: &[u8]) -> Option<i64> {
    read_number(data, 5).filter(|value| number_bytes(*value) == data)
}

/// The number a word for one stands for.
pub(crate) fn small_number(code: u8) -> Option<i64> {
    match code {
        ZERO => Some(0),
        NEGATIVE_ONE => Some(-1),
        ONE..=SIXTEEN => Some(i64::from(code - ONE) + 1),
        _ => None,
    }
}

/// Does data before this word read better as a number?
pub(crate) fn takes_number(code: Option<u8>) -> bool {
    matches!(code, Some(ADD_1..=WITHIN | PICK | ROLL | CHECK_HEIGHT | CHECK_SEQUENCE))
}

/// The words of a gamma script. A byte that has no word shows as `?xx`.
pub(crate) fn words_of(script: &[u8]) -> String {
    let mut out = String::new();
    let mut at = 0;
    while let Some(code) = script.get(at).copied() {
        if !out.is_empty() {
            out.push(' ');
        }
        at += 1;
        if let Some(number) = small_number(code) {
            let _ = write!(out, "{number}");
            continue;
        }
        if code <= PUSH_DATA_4 {
            let Some((data, next)) = read_push(script, at, code) else {
                out.push_str("?cut");
                break;
            };
            match number_of(data).filter(|_| data.len() == 1 || takes_number(script.get(next).copied())) {
                Some(number) => {
                    let _ = write!(out, "{number}");
                }
                None => show_data(data, &mut out),
            }
            at = next;
            continue;
        }
        match NAMES.iter().find(|(_, found)| *found == code) {
            Some((name, _)) => out.push_str(name),
            None => {
                let _ = write!(out, "?{code:02x}");
            }
        }
    }
    out
}
