//! Gamma: Bitcoin Script as it runs today, without Taproot. Bitcoin's enabled words with its
//! numbers and names, its script numbers (little-endian, sign in the top bit, four bytes into
//! arithmetic), its signature kinds, multi-signature up to 20 keys, and its relative lock
//! (CHECKSEQUENCEVERIFY, BIP 112). A coin can be spent from gamma only from its upgrade height.
//!
//! Where OLC differs, and why:
//! - `HASH` (0xa9) is OLC's address hash, as in beta, so a lock reads the same in both; Bitcoin's
//!   HASH160 sits at 0xc0.
//! - `CHECKHEIGHT` (0xb1, Bitcoin's CHECKLOCKTIMEVERIFY byte) reads the block height, as in beta.
//!   The transaction's own lock time is checked by the chain for every transaction.
//! - Relative locks and lock times count blocks only: boards keep no clock.
//! - A multi-signature takes no extra value, Bitcoin's old off-by-one.
//! - CODESEPARATOR and SHA1 are left out, and a script fails where it meets them.
//! - The limits are a board's: 200 steps, 100 values on both stacks, 520 bytes a value.
use crate::GAMMA;
use alloc::vec::Vec;
use olc_core::types::{SEQUENCE_BLOCKS, SEQUENCE_BY_TIME, SEQUENCE_DISABLE, UNLOCK_FREE_VERSION};
use olc_core::{LockLanguage, SpendContext, Spending};
use olc_crypto::{hash, ripemd160, sha256, verify};

pub mod op {
    pub const ZERO: u8 = 0x00;
    pub const PUSH_MAX: u8 = 0x4b;
    pub const PUSH_DATA_1: u8 = 0x4c;
    pub const PUSH_DATA_2: u8 = 0x4d;
    pub const PUSH_DATA_4: u8 = 0x4e;
    pub const NEGATIVE_ONE: u8 = 0x4f;
    pub const ONE: u8 = 0x51;
    pub const SIXTEEN: u8 = 0x60;
    pub const NOP: u8 = 0x61;
    pub const IF: u8 = 0x63;
    pub const NOT_IF: u8 = 0x64;
    pub const ELSE: u8 = 0x67;
    pub const END_IF: u8 = 0x68;
    pub const VERIFY: u8 = 0x69;
    pub const RETURN: u8 = 0x6a;
    pub const TO_ALT: u8 = 0x6b;
    pub const FROM_ALT: u8 = 0x6c;
    pub const DROP_2: u8 = 0x6d;
    pub const DUP_2: u8 = 0x6e;
    pub const DUP_3: u8 = 0x6f;
    pub const OVER_2: u8 = 0x70;
    pub const ROT_2: u8 = 0x71;
    pub const SWAP_2: u8 = 0x72;
    pub const IF_DUP: u8 = 0x73;
    pub const DEPTH: u8 = 0x74;
    pub const DROP: u8 = 0x75;
    pub const DUP: u8 = 0x76;
    pub const NIP: u8 = 0x77;
    pub const OVER: u8 = 0x78;
    pub const PICK: u8 = 0x79;
    pub const ROLL: u8 = 0x7a;
    pub const ROT: u8 = 0x7b;
    pub const SWAP: u8 = 0x7c;
    pub const TUCK: u8 = 0x7d;
    pub const SIZE: u8 = 0x82;
    pub const EQUAL: u8 = 0x87;
    pub const EQUAL_VERIFY: u8 = 0x88;
    pub const ADD_1: u8 = 0x8b;
    pub const SUB_1: u8 = 0x8c;
    pub const NEGATE: u8 = 0x8f;
    pub const ABS: u8 = 0x90;
    pub const NOT: u8 = 0x91;
    pub const NOT_ZERO: u8 = 0x92;
    pub const ADD: u8 = 0x93;
    pub const SUB: u8 = 0x94;
    pub const BOOL_AND: u8 = 0x9a;
    pub const BOOL_OR: u8 = 0x9b;
    pub const NUM_EQUAL: u8 = 0x9c;
    pub const NUM_EQUAL_VERIFY: u8 = 0x9d;
    pub const NUM_NOT_EQUAL: u8 = 0x9e;
    pub const LESS: u8 = 0x9f;
    pub const GREATER: u8 = 0xa0;
    pub const LESS_OR_EQUAL: u8 = 0xa1;
    pub const GREATER_OR_EQUAL: u8 = 0xa2;
    pub const MIN: u8 = 0xa3;
    pub const MAX: u8 = 0xa4;
    pub const WITHIN: u8 = 0xa5;
    pub const RIPEMD160: u8 = 0xa6;
    pub const SHA256: u8 = 0xa8;
    /// OLC's address hash, as in beta.
    pub const HASH: u8 = 0xa9;
    pub const HASH256: u8 = 0xaa;
    pub const CHECK_SIG: u8 = 0xac;
    pub const CHECK_SIG_VERIFY: u8 = 0xad;
    pub const CHECK_MULTI_SIG: u8 = 0xae;
    pub const CHECK_MULTI_SIG_VERIFY: u8 = 0xaf;
    pub const NOP1: u8 = 0xb0;
    /// Fails until the block height reaches the top value, as in beta.
    pub const CHECK_HEIGHT: u8 = 0xb1;
    pub const CHECK_SEQUENCE: u8 = 0xb2;
    pub const NOP4: u8 = 0xb3;
    pub const NOP10: u8 = 0xb9;
    /// Bitcoin's HASH160: RIPEMD-160 of SHA-256.
    pub const HASH160: u8 = 0xc0;
}

/// Words Bitcoin turned off. A script holding one fails even where it does not run, as there.
const DISABLED: &[u8] = &[
    0x65, 0x66, 0x7e, 0x7f, 0x80, 0x81, 0x83, 0x84, 0x85, 0x86, 0x8d, 0x8e, 0x95, 0x96, 0x97, 0x98, 0x99,
];

const MAX_OPS: usize = 200;
const MAX_STACK: usize = 100;
const MAX_VALUE: usize = 520;
/// Bitcoin's most keys in one multi-signature.
pub const MAX_KEYS: i64 = 20;
const SIGNATURE: usize = 64;

pub struct Gamma;

impl LockLanguage for Gamma {
    fn id(&self) -> u8 {
        GAMMA
    }

    fn name(&self) -> &'static str {
        "gamma"
    }

    fn check(&self, lock: &[u8], unlock: &[u8], context: &SpendContext) -> bool {
        // Before the upgrade nothing is spent from gamma, as older builds do not know it.
        let Some(spending) = context.spending else {
            return false;
        };
        let mut machine = Machine { stack: Vec::new(), alt: Vec::new(), ops: 0, context, spending };
        machine.run(unlock).is_some()
            && machine.run(lock).is_some()
            && machine.stack.last().is_some_and(|top| is_true(top))
    }

    fn signature_checks(&self, lock: &[u8], unlock: &[u8]) -> usize {
        signature_checks(unlock).saturating_add(signature_checks(lock))
    }
}

/// The data a push at `at` carries, and where the next word starts.
pub fn read_push(script: &[u8], at: usize, code: u8) -> Option<(&[u8], usize)> {
    let (length, start) = match code {
        op::ZERO..=op::PUSH_MAX => (usize::from(code), at),
        op::PUSH_DATA_1 => (usize::from(*script.get(at)?), at + 1),
        op::PUSH_DATA_2 => {
            let bytes = script.get(at..at + 2)?;
            (usize::from(u16::from_le_bytes([bytes[0], bytes[1]])), at + 2)
        }
        op::PUSH_DATA_4 => {
            let bytes = script.get(at..at + 4)?;
            let length = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            (usize::try_from(length).ok()?, at + 4)
        }
        _ => return None,
    };
    let end = start.checked_add(length)?;
    Some((script.get(start..end)?, end))
}

/// The most signature checks `script` can cost, counted as Bitcoin counts them: one for a single
/// signature, and for a multi-signature the key count when a small number word stands right
/// before it, else 20.
pub fn signature_checks(script: &[u8]) -> usize {
    let mut checks = 0usize;
    let mut last = None;
    let mut at = 0;
    while let Some(code) = script.get(at).copied() {
        at += 1;
        if code <= op::PUSH_DATA_4 {
            match read_push(script, at, code) {
                Some((_, next)) => at = next,
                None => break,
            }
        }
        let cost = match (code, last) {
            (op::CHECK_SIG | op::CHECK_SIG_VERIFY, _) => 1,
            (op::CHECK_MULTI_SIG | op::CHECK_MULTI_SIG_VERIFY, Some(small @ op::ONE..=op::SIXTEEN)) => {
                usize::from(small - op::ONE + 1)
            }
            (op::CHECK_MULTI_SIG | op::CHECK_MULTI_SIG_VERIFY, _) => 20,
            _ => 0,
        };
        checks = checks.saturating_add(cost);
        last = Some(code);
    }
    checks
}

/// Bitcoin's truth: any byte not zero, except a lone sign bit at the end (negative zero).
pub fn is_true(value: &[u8]) -> bool {
    match value.split_last() {
        Some((last, rest)) => rest.iter().any(|byte| *byte != 0) || (*last != 0 && *last != 0x80),
        None => false,
    }
}

/// A script number of at most `most` bytes: little-endian, the sign in the top bit.
pub fn read_number(value: &[u8], most: usize) -> Option<i64> {
    if value.len() > most {
        return None;
    }
    let Some((last, _)) = value.split_last() else {
        return Some(0);
    };
    let mut wide = [0u8; 8];
    wide[..value.len()].copy_from_slice(value);
    wide[value.len() - 1] = last & 0x7f;
    let magnitude = i64::from_le_bytes(wide);
    Some(if last & 0x80 != 0 { -magnitude } else { magnitude })
}

/// The shortest bytes of a script number.
pub fn number_bytes(value: i64) -> Vec<u8> {
    let magnitude = value.unsigned_abs().to_le_bytes();
    let length = magnitude.iter().rposition(|byte| *byte != 0).map_or(0, |at| at + 1);
    let mut out = magnitude[..length].to_vec();
    match out.last_mut() {
        Some(top) if *top & 0x80 != 0 => out.push(if value < 0 { 0x80 } else { 0 }),
        Some(top) if value < 0 => *top |= 0x80,
        _ => {}
    }
    out
}

struct Machine<'a> {
    stack: Vec<Vec<u8>>,
    alt: Vec<Vec<u8>>,
    ops: usize,
    context: &'a SpendContext<'a>,
    spending: Spending<'a>,
}

impl Machine<'_> {
    fn pop(&mut self) -> Option<Vec<u8>> {
        self.stack.pop()
    }

    fn pop_number(&mut self) -> Option<i64> {
        read_number(&self.pop()?, 4)
    }

    fn push_number(&mut self, value: i64) {
        self.stack.push(number_bytes(value));
    }

    fn push_bool(&mut self, value: bool) {
        self.push_number(i64::from(value));
    }

    /// The value `depth` places down, the top being 1.
    fn peek(&self, depth: usize) -> Option<Vec<u8>> {
        let at = self.stack.len().checked_sub(depth)?;
        self.stack.get(at).cloned()
    }

    /// Takes out the value `depth` places down.
    fn take(&mut self, depth: usize) -> Option<Vec<u8>> {
        let at = self.stack.len().checked_sub(depth)?;
        (at < self.stack.len()).then(|| self.stack.remove(at))
    }

    /// Returns `None` when the script fails.
    fn run(&mut self, script: &[u8]) -> Option<()> {
        self.alt.clear();
        let mut branches: Vec<bool> = Vec::new();
        let mut at = 0;
        while let Some(code) = script.get(at).copied() {
            at += 1;
            self.ops += 1;
            if self.ops > MAX_OPS || DISABLED.contains(&code) {
                return None;
            }
            let running = branches.iter().all(|branch| *branch);
            if code <= op::PUSH_DATA_4 {
                let (data, next) = read_push(script, at, code)?;
                if data.len() > MAX_VALUE {
                    return None;
                }
                if running {
                    self.stack.push(data.to_vec());
                }
                at = next;
            } else {
                match code {
                    op::IF | op::NOT_IF => {
                        let taken = running && is_true(&self.pop()?) == (code == op::IF);
                        branches.push(taken);
                    }
                    op::ELSE => {
                        let last = branches.last_mut()?;
                        *last = !*last;
                    }
                    op::END_IF => {
                        branches.pop()?;
                    }
                    _ if running => self.execute(code)?,
                    _ => {}
                }
            }
            if self.stack.len() + self.alt.len() > MAX_STACK {
                return None;
            }
        }
        branches.is_empty().then_some(())
    }

    fn execute(&mut self, code: u8) -> Option<()> {
        match code {
            op::NEGATIVE_ONE => self.push_number(-1),
            op::ONE..=op::SIXTEEN => self.push_number(i64::from(code - op::ONE + 1)),
            op::NOP | op::NOP1 | op::NOP4..=op::NOP10 => {}
            op::VERIFY => self.verify()?,
            op::TO_ALT => {
                let top = self.pop()?;
                self.alt.push(top);
            }
            op::FROM_ALT => {
                let top = self.alt.pop()?;
                self.stack.push(top);
            }
            op::DROP_2 => {
                self.pop()?;
                self.pop()?;
            }
            op::DUP_2 | op::DUP_3 | op::OVER_2 => {
                let (count, depth) = match code {
                    op::DUP_2 => (2, 2),
                    op::DUP_3 => (3, 3),
                    _ => (2, 4),
                };
                for _ in 0..count {
                    let copy = self.peek(depth)?;
                    self.stack.push(copy);
                }
            }
            op::ROT_2 => {
                let first = self.take(6)?;
                let second = self.take(5)?;
                self.stack.extend([first, second]);
            }
            op::SWAP_2 => {
                let first = self.take(4)?;
                let second = self.take(3)?;
                self.stack.extend([first, second]);
            }
            op::IF_DUP => {
                let top = self.peek(1)?;
                if is_true(&top) {
                    self.stack.push(top);
                }
            }
            op::DEPTH => self.push_number(i64::try_from(self.stack.len()).ok()?),
            op::DROP => {
                self.pop()?;
            }
            op::DUP => {
                let top = self.peek(1)?;
                self.stack.push(top);
            }
            op::NIP => {
                self.take(2)?;
            }
            op::OVER => {
                let second = self.peek(2)?;
                self.stack.push(second);
            }
            op::PICK | op::ROLL => {
                let depth = usize::try_from(self.pop_number()?).ok()?.checked_add(1)?;
                let value = match code {
                    op::PICK => self.peek(depth)?,
                    _ => self.take(depth)?,
                };
                self.stack.push(value);
            }
            op::ROT => {
                let third = self.take(3)?;
                self.stack.push(third);
            }
            op::SWAP => {
                let second = self.take(2)?;
                self.stack.push(second);
            }
            op::TUCK => {
                let top = self.peek(1)?;
                let at = self.stack.len().checked_sub(2)?;
                self.stack.insert(at, top);
            }
            op::SIZE => {
                let length = self.peek(1)?.len();
                self.push_number(i64::try_from(length).ok()?);
            }
            op::EQUAL | op::EQUAL_VERIFY => {
                let same = self.pop()? == self.pop()?;
                self.push_bool(same);
                if code == op::EQUAL_VERIFY {
                    self.verify()?;
                }
            }
            op::ADD_1..=op::NOT_ZERO => self.unary(code)?,
            op::ADD..=op::MAX => self.binary(code)?,
            op::WITHIN => {
                let most = self.pop_number()?;
                let least = self.pop_number()?;
                let value = self.pop_number()?;
                self.push_bool(least <= value && value < most);
            }
            op::RIPEMD160 | op::SHA256 | op::HASH | op::HASH256 | op::HASH160 => {
                let top = self.pop()?;
                let digest = match code {
                    op::RIPEMD160 => ripemd160(&top).to_vec(),
                    op::SHA256 => sha256(&top).to_vec(),
                    op::HASH => hash(&top)[..20].to_vec(),
                    op::HASH256 => hash(&top).to_vec(),
                    _ => ripemd160(&sha256(&top)).to_vec(),
                };
                self.stack.push(digest);
            }
            op::CHECK_SIG | op::CHECK_SIG_VERIFY => {
                let key = self.pop()?;
                let signature = self.pop()?;
                self.push_bool(self.signed(&signature, &key));
                if code == op::CHECK_SIG_VERIFY {
                    self.verify()?;
                }
            }
            op::CHECK_MULTI_SIG | op::CHECK_MULTI_SIG_VERIFY => {
                let valid = self.check_multi_sig()?;
                self.push_bool(valid);
                if code == op::CHECK_MULTI_SIG_VERIFY {
                    self.verify()?;
                }
            }
            op::CHECK_HEIGHT => {
                let height = read_number(&self.peek(1)?, 5)?;
                if height < 0 || self.context.height < height.unsigned_abs() {
                    return None;
                }
            }
            op::CHECK_SEQUENCE => self.check_sequence()?,
            _ => return None,
        }
        Some(())
    }

    fn verify(&mut self) -> Option<()> {
        is_true(&self.pop()?).then_some(())
    }

    fn unary(&mut self, code: u8) -> Option<()> {
        let value = self.pop_number()?;
        let result = match code {
            op::ADD_1 => value + 1,
            op::SUB_1 => value - 1,
            op::NEGATE => -value,
            op::ABS => value.abs(),
            op::NOT => i64::from(value == 0),
            op::NOT_ZERO => i64::from(value != 0),
            _ => return None,
        };
        self.push_number(result);
        Some(())
    }

    fn binary(&mut self, code: u8) -> Option<()> {
        let second = self.pop_number()?;
        let first = self.pop_number()?;
        let result = match code {
            op::ADD => first + second,
            op::SUB => first - second,
            op::BOOL_AND => i64::from(first != 0 && second != 0),
            op::BOOL_OR => i64::from(first != 0 || second != 0),
            op::NUM_EQUAL | op::NUM_EQUAL_VERIFY => i64::from(first == second),
            op::NUM_NOT_EQUAL => i64::from(first != second),
            op::LESS => i64::from(first < second),
            op::GREATER => i64::from(first > second),
            op::LESS_OR_EQUAL => i64::from(first <= second),
            op::GREATER_OR_EQUAL => i64::from(first >= second),
            op::MIN => first.min(second),
            op::MAX => first.max(second),
            _ => return None,
        };
        self.push_number(result);
        if code == op::NUM_EQUAL_VERIFY {
            self.verify()?;
        }
        Some(())
    }

    /// A signature of 64 bytes covers what kind ALL does; a 65th byte names its kind.
    fn signed(&self, signature: &[u8], key: &[u8]) -> bool {
        let digest = match signature.len() {
            SIGNATURE => Some(self.context.signature_hash),
            65 => self.spending.signature_hash(signature[SIGNATURE]),
            _ => None,
        };
        let bare = signature.get(..SIGNATURE).unwrap_or_default();
        digest.is_some_and(|digest| verify(key, &digest, bare))
    }

    /// Keys, key count, signatures and signature count, each signature by a later key than the
    /// one before, as in Bitcoin.
    fn check_multi_sig(&mut self) -> Option<bool> {
        let key_count = self.pop_number()?;
        if !(0..=MAX_KEYS).contains(&key_count) {
            return None;
        }
        let keys: Vec<Vec<u8>> = (0..key_count).map(|_| self.pop()).collect::<Option<_>>()?;
        let signature_count = self.pop_number()?;
        if !(0..=key_count).contains(&signature_count) {
            return None;
        }
        let signatures: Vec<Vec<u8>> = (0..signature_count).map(|_| self.pop()).collect::<Option<_>>()?;
        let mut remaining = keys.iter();
        Some(signatures.iter().all(|signature| remaining.any(|key| self.signed(signature, key))))
    }

    /// BIP 112, in blocks: the input's sequence must lock at least the top value.
    fn check_sequence(&mut self) -> Option<()> {
        let wanted = read_number(&self.peek(1)?, 5)?;
        let wanted = u64::try_from(wanted).ok()?;
        if wanted & u64::from(SEQUENCE_DISABLE) != 0 {
            return Some(());
        }
        let sequence = self.spending.sequence();
        let by_time = u64::from(SEQUENCE_BY_TIME);
        let refused = self.spending.tx.version < UNLOCK_FREE_VERSION
            || sequence & SEQUENCE_DISABLE != 0
            || wanted & by_time != 0
            || sequence & SEQUENCE_BY_TIME != 0
            || wanted & u64::from(SEQUENCE_BLOCKS) > u64::from(sequence & SEQUENCE_BLOCKS);
        (!refused).then_some(())
    }
}

#[cfg(test)]
#[path = "gamma_tests.rs"]
mod tests;
