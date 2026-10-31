//! A stack language at the level of early Bitcoin Script, using the same opcode numbers.
//!
//! The unlock script runs first and the lock script runs on the stack it leaves. The output can
//! be spent when the last value on the stack is true.
use crate::BETA;
use alloc::vec::Vec;
use olc_core::{LockLanguage, SpendContext};
use olc_crypto::{hash, verify};

pub mod op {
    pub const ZERO: u8 = 0x00;
    /// 0x01 to 0x4b push that many following bytes.
    pub const PUSH_MAX: u8 = 0x4b;
    pub const ONE: u8 = 0x51;
    pub const IF: u8 = 0x63;
    pub const ELSE: u8 = 0x67;
    pub const END_IF: u8 = 0x68;
    pub const VERIFY: u8 = 0x69;
    pub const RETURN: u8 = 0x6a;
    pub const DROP: u8 = 0x75;
    pub const DUP: u8 = 0x76;
    pub const SWAP: u8 = 0x7c;
    pub const EQUAL: u8 = 0x87;
    pub const EQUAL_VERIFY: u8 = 0x88;
    pub const ADD: u8 = 0x93;
    /// Replaces the top value with its address hash.
    pub const HASH: u8 = 0xa9;
    pub const CHECK_SIG: u8 = 0xac;
    /// Keys, key count, signatures and signature count, as in Bitcoin without its extra pop.
    pub const CHECK_MULTI_SIG: u8 = 0xae;
    /// Fails until the block height reaches the top value.
    pub const CHECK_HEIGHT: u8 = 0xb1;
}

const MAX_OPS: usize = 200;
const MAX_STACK: usize = 100;

pub struct Beta;

impl LockLanguage for Beta {
    fn id(&self) -> u8 {
        BETA
    }

    fn name(&self) -> &'static str {
        "beta"
    }

    fn check(&self, lock: &[u8], unlock: &[u8], context: &SpendContext) -> bool {
        let mut machine = Machine {
            stack: Vec::new(),
            ops: 0,
            context,
        };
        machine.run(unlock).is_some()
            && machine.run(lock).is_some()
            && machine.stack.last().is_some_and(|top| is_true(top))
    }
}

struct Machine<'a> {
    stack: Vec<Vec<u8>>,
    ops: usize,
    context: &'a SpendContext,
}

fn is_true(value: &[u8]) -> bool {
    value.iter().any(|byte| *byte != 0)
}

fn number(value: &[u8]) -> Option<u64> {
    if value.len() > 8 {
        return None;
    }
    let mut bytes = [0u8; 8];
    bytes[..value.len()].copy_from_slice(value);
    Some(u64::from_le_bytes(bytes))
}

impl Machine<'_> {
    fn pop(&mut self) -> Option<Vec<u8>> {
        self.stack.pop()
    }

    fn push_bool(&mut self, value: bool) {
        self.stack
            .push(if value { alloc::vec![1] } else { Vec::new() });
    }

    /// Returns `None` when the script fails.
    fn run(&mut self, script: &[u8]) -> Option<()> {
        // Each entry says whether the branch it opened is running.
        let mut branches: Vec<bool> = Vec::new();
        let mut at = 0;
        while at < script.len() {
            self.ops += 1;
            if self.ops > MAX_OPS || self.stack.len() > MAX_STACK {
                return None;
            }
            let code = script[at];
            at += 1;
            let running = branches.iter().all(|branch| *branch);
            if (1..=op::PUSH_MAX).contains(&code) {
                let end = at + usize::from(code);
                let data = script.get(at..end)?;
                if running {
                    self.stack.push(data.to_vec());
                }
                at = end;
                continue;
            }
            match code {
                op::IF => {
                    let taken = running && is_true(&self.pop()?);
                    branches.push(taken);
                    continue;
                }
                op::ELSE => {
                    let last = branches.last_mut()?;
                    *last = !*last;
                    continue;
                }
                op::END_IF => {
                    branches.pop()?;
                    continue;
                }
                _ => {}
            }
            if running {
                self.execute(code)?;
            }
        }
        branches.is_empty().then_some(())
    }

    fn execute(&mut self, code: u8) -> Option<()> {
        match code {
            op::ZERO => self.stack.push(Vec::new()),
            op::ONE => self.stack.push(alloc::vec![1]),
            op::VERIFY => {
                if !is_true(&self.pop()?) {
                    return None;
                }
            }
            op::RETURN => return None,
            op::DROP => {
                self.pop()?;
            }
            op::DUP => {
                let top = self.stack.last()?.clone();
                self.stack.push(top);
            }
            op::SWAP => {
                let length = self.stack.len();
                if length < 2 {
                    return None;
                }
                self.stack.swap(length - 1, length - 2);
            }
            op::EQUAL | op::EQUAL_VERIFY => {
                let same = self.pop()? == self.pop()?;
                if code == op::EQUAL {
                    self.push_bool(same);
                } else if !same {
                    return None;
                }
            }
            op::ADD => {
                let sum = number(&self.pop()?)?.checked_add(number(&self.pop()?)?)?;
                self.stack.push(sum.to_le_bytes().to_vec());
            }
            op::HASH => {
                let top = self.pop()?;
                self.stack.push(hash(&top)[..20].to_vec());
            }
            op::CHECK_SIG => {
                let public_key = self.pop()?;
                let signature = self.pop()?;
                let valid = verify(&public_key, &self.context.signature_hash, &signature);
                self.push_bool(valid);
            }
            op::CHECK_MULTI_SIG => {
                let valid = self.check_multi_sig()?;
                self.push_bool(valid);
            }
            op::CHECK_HEIGHT => {
                let height = number(self.stack.last()?)?;
                if self.context.height < height {
                    return None;
                }
            }
            _ => return None,
        }
        Some(())
    }

    fn check_multi_sig(&mut self) -> Option<bool> {
        let key_count = number(&self.pop()?)? as usize;
        let mut keys = Vec::new();
        for _ in 0..key_count {
            keys.push(self.pop()?);
        }
        let signature_count = number(&self.pop()?)? as usize;
        let mut signatures = Vec::new();
        for _ in 0..signature_count {
            signatures.push(self.pop()?);
        }
        let digest = &self.context.signature_hash;
        let mut remaining = keys.iter();
        Some(
            signatures
                .iter()
                .all(|signature| remaining.any(|key| verify(key, digest, signature))),
        )
    }
}

/// The lock script for a plain payment, the same shape as Bitcoin's pay-to-public-key-hash.
pub fn pay_to_address(address: &[u8; 20]) -> Vec<u8> {
    let mut script = alloc::vec![op::DUP, op::HASH, 20];
    script.extend_from_slice(address);
    script.extend_from_slice(&[op::EQUAL_VERIFY, op::CHECK_SIG]);
    script
}

/// The unlock script for [`pay_to_address`].
pub fn unlock_address(public_key: &[u8; 33], signature: &[u8; 64]) -> Vec<u8> {
    let mut script = alloc::vec![64];
    script.extend_from_slice(signature);
    script.push(33);
    script.extend_from_slice(public_key);
    script
}

#[cfg(test)]
mod tests {
    use super::*;
    use olc_crypto::{address_of, public_key, sign};

    fn context(height: u64) -> SpendContext {
        SpendContext {
            signature_hash: hash(b"spend"),
            height,
        }
    }

    fn check(lock: &[u8], unlock: &[u8], height: u64) -> bool {
        Beta.check(lock, unlock, &context(height))
    }

    fn key(seed: u8) -> ([u8; 32], [u8; 33]) {
        let secret = [seed; 32];
        (secret, public_key(&secret).unwrap())
    }

    #[test]
    fn address_payment() {
        let (secret, public) = key(1);
        let signature = sign(&secret, &hash(b"spend")).unwrap();
        let lock = pay_to_address(&address_of(&public));
        assert!(check(&lock, &unlock_address(&public, &signature), 0));

        let (other_secret, other_public) = key(2);
        let wrong = sign(&other_secret, &hash(b"spend")).unwrap();
        assert!(!check(&lock, &unlock_address(&other_public, &wrong), 0));
    }

    #[test]
    fn hash_puzzle() {
        let mut lock = alloc::vec![op::HASH, 20];
        lock.extend_from_slice(&hash(b"secret")[..20]);
        lock.push(op::EQUAL);
        let mut unlock = alloc::vec![6];
        unlock.extend_from_slice(b"secret");
        assert!(check(&lock, &unlock, 0));
        let mut wrong = alloc::vec![5];
        wrong.extend_from_slice(b"wrong");
        assert!(!check(&lock, &wrong, 0));
    }

    #[test]
    fn height_lock_opens_at_the_height() {
        let lock = [1, 10, op::CHECK_HEIGHT];
        assert!(!check(&lock, &[], 9));
        assert!(check(&lock, &[], 10));
    }

    #[test]
    fn two_of_two_needs_both_signatures() {
        let (secret_a, public_a) = key(3);
        let (secret_b, public_b) = key(4);
        let digest = hash(b"spend");
        let signature_a = sign(&secret_a, &digest).unwrap();
        let signature_b = sign(&secret_b, &digest).unwrap();

        let mut lock = alloc::vec![1, 2, 33];
        lock.extend_from_slice(&public_b);
        lock.push(33);
        lock.extend_from_slice(&public_a);
        lock.extend_from_slice(&[1, 2, op::CHECK_MULTI_SIG]);

        let unlock = |first: &[u8; 64], second: &[u8; 64]| {
            let mut script = alloc::vec![64];
            script.extend_from_slice(second);
            script.push(64);
            script.extend_from_slice(first);
            script
        };
        // Keys pop in the order A, B, so signatures must come in that order.
        assert!(check(&lock, &unlock(&signature_a, &signature_b), 0));
        assert!(!check(&lock, &unlock(&signature_a, &signature_a), 0));
    }

    #[test]
    fn if_else_picks_a_branch() {
        let lock = [op::IF, op::ONE, op::ELSE, op::ZERO, op::END_IF];
        assert!(check(&lock, &[op::ONE], 0));
        assert!(!check(&lock, &[op::ZERO], 0));
    }

    #[test]
    fn unknown_opcode_fails() {
        assert!(!check(&[op::ONE, 0xff], &[], 0));
    }
}
