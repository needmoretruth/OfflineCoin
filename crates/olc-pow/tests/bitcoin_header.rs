//! A block of the new OfflineCoin, worked as a miner built for Bitcoin works it: from its 80
//! header bytes alone, with a SHA-256 of its own, a target read from `nBits` and the hash
//! compared word by word from the top, as cpuminer's `fulltest` does. It must find the id the
//! node finds, and the node must take the block. Bitcoin's own first block reads the same way.
use olc_core::{coinbase_split, Address, Block, Chain, CoinConfig, Plugins, EXTRANONCE_BYTES};
use olc_pow::ProofOfWork;

/// SHA-256 from FIPS 180-4, apart from the node's own.
fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut state: [u32; 8] =
        [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&(data.len() as u64 * 8).to_be_bytes());
    for block in message.chunks(64) {
        let mut w = [0u32; 64];
        for (index, word) in block.chunks(4).enumerate() {
            w[index] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choose = (e & f) ^ (!e & g);
            let t1 = h.wrapping_add(s1).wrapping_add(choose).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(majority);
            (h, g, f, e, d, c, b, a) = (g, f, e, d.wrapping_add(t1), c, b, a, t1.wrapping_add(t2));
        }
        for (slot, add) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(add);
        }
    }
    let mut out = [0u8; 32];
    for (index, word) in state.iter().enumerate() {
        out[4 * index..4 * index + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

fn sha256d(data: &[u8]) -> [u8; 32] {
    sha256(&sha256(data))
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len()).step_by(2).map(|at| u8::from_str_radix(&text[at..at + 2], 16).unwrap()).collect()
}

/// cpuminer's `fulltest`: the hash and the target as eight 32-bit words, compared from the top.
fn full_test(hash: &[u8; 32], bits: u32) -> bool {
    let (size, mantissa) = ((bits >> 24) as usize, bits & 0x007f_ffff);
    let mut target = [0u8; 32];
    for (index, byte) in mantissa.to_le_bytes()[..3].iter().enumerate() {
        if let Some(slot) = (size + index).checked_sub(3).and_then(|at| target.get_mut(at)) {
            *slot = *byte;
        }
    }
    for word in (0..8).rev() {
        let read = |bytes: &[u8; 32]| u32::from_le_bytes(bytes[4 * word..4 * word + 4].try_into().unwrap());
        let (ours, limit) = (read(hash), read(&target));
        if ours != limit {
            return ours < limit;
        }
    }
    true
}

/// What a miner reads from the 80 bytes it was given.
fn fields(bytes: &[u8]) -> (u32, u32, u32) {
    let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    (word(0), word(68), word(72))
}

#[test]
fn bitcoins_first_block_reads_as_bitcoin_reads_it() {
    let bytes = unhex(concat!(
        "0100000000000000000000000000000000000000000000000000000000000000000000003ba3edfd7a7b12b27a",
        "c72c3e67768f617fc81bc3888a51323a9fb8aa4b1e5e4a29ab5f49ffff001d1dac2b7c"
    ));
    let header = olc_core::BlockHeader::from_bitcoin_bytes(&bytes).unwrap();
    assert_eq!(header.bitcoin_bytes().as_slice(), bytes.as_slice());
    assert_eq!(header.time_ms, 1_231_006_505_000);
    let mut shown = sha256d(&bytes);
    shown.reverse();
    let shown: String = shown.iter().map(|byte| format!("{byte:02x}")).collect();
    assert_eq!(shown, "000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f");
    assert!(full_test(&sha256d(&bytes), 0x1d00_ffff));
}

#[test]
fn a_new_block_hashes_from_its_80_bytes_to_the_id_the_node_finds() {
    let coin = CoinConfig::olc();
    let mut chain = Chain::new(coin.clone(), Plugins { consensus: Box::new(ProofOfWork), languages: Vec::new() });
    let miner: Address = [7; 20];
    let mut block: Block = chain.block_template(&miner, &[], coin.genesis_time_ms + 30_000);

    // A pool fills the extranonce between coinb1 and coinb2, and the root follows.
    let (coinb1, coinb2) = coinbase_split(&block.txs[0]).unwrap();
    let extranonce = [1, 2, 3, 4, 5, 6, 7, 8];
    assert_eq!(extranonce.len(), EXTRANONCE_BYTES);
    let coinbase_id = sha256d(&[coinb1.as_slice(), &extranonce, &coinb2].concat());
    block.txs[0].outputs.last_mut().unwrap().lock = extranonce.to_vec();
    assert_eq!(block.txs[0].id(), coinbase_id);
    block.header.merkle_root = coinbase_id;

    assert!(olc_core::Consensus::try_seal(&ProofOfWork, &mut block.header, 50_000_000));
    let bytes = block.header.bitcoin_bytes();
    let (version, time, bits) = fields(&bytes);
    assert_eq!(version >> 29, 1, "BIP 9's top bits");
    assert_eq!(u64::from(time) * 1_000, block.header.time_ms);
    assert_eq!(bits, olc_core::bits::bits_of(block.header.difficulty));
    let hash = sha256d(&bytes);
    assert_eq!(hash, block.id(), "the miner's hash is the node's id");
    assert!(full_test(&hash, bits));
    chain.add_block(block).unwrap();
    assert_eq!(chain.height(), 1);
}
