//! Parsers that hold no chain: what any radio, socket or cable hands a node first.
use crate::fixture::encoded_messages;
use crate::{Parse, Target};
use olc_core::{BlockHeader, CoinConfig};
use olc_node::bluetooth::{node_in_advert, node_of, payload};
use olc_node::fragment::{split, Reassembler};
use olc_node::frame::{self, Decoder};
use olc_node::line::{self, Heard, Lines};
use olc_node::network::{read, stamp, OPEN};
use olc_node::{Hello, Message};

const ME: [u8; 6] = [2; 6];
const PIECE: usize = 240;

fn messages_stamped() -> Vec<Vec<u8>> {
    let mut seeds = encoded_messages();
    seeds.extend(encoded_messages().iter().map(|message| stamp(OPEN, message)));
    seeds
}

fn read_network(bytes: &[u8]) {
    if let Some((_, rest)) = read(bytes) {
        crate::taken();
        std::hint::black_box(rest.len());
    }
}

fn decode_message(bytes: &[u8]) {
    if let Some(message) = Message::decode(bytes) {
        crate::taken();
        std::hint::black_box(message.encode());
    }
}

fn frames() -> Vec<Vec<u8>> {
    encoded_messages().iter().map(|message| frame::encode(message)).collect()
}

/// A cut in the middle of a stream is how bytes really arrive.
fn start_decoder() -> Parse {
    let mut decoder = Decoder::default();
    Box::new(move |bytes| {
        let (first, second) = bytes.split_at(bytes.len() / 2);
        for part in [first, second] {
            decoder.push(part);
            while let Some(message) = decoder.next_message() {
                crate::taken();
                std::hint::black_box(message.len());
            }
        }
        if decoder.is_broken() {
            decoder = Decoder::default();
        }
    })
}

fn pieces() -> Vec<Vec<u8>> {
    let mut seeds = Vec::new();
    for message in encoded_messages() {
        seeds.extend(split(1, &message, PIECE));
    }
    seeds
}

fn start_reassembler() -> Parse {
    let mut reassembler = Reassembler::default();
    Box::new(move |bytes| {
        let source = u32::from(bytes.first().copied().unwrap_or(0) & 15);
        if reassembler.push(source, bytes).is_some() {
            crate::taken();
        }
    })
}

fn lines() -> Vec<Vec<u8>> {
    let mut seeds: Vec<Vec<u8>> =
        encoded_messages().iter().map(|message| line::encode(message)).collect();
    seeds.push(b"status height=1\n".to_vec());
    seeds
}

fn start_lines() -> Parse {
    let mut lines = Lines::default();
    Box::new(move |bytes| {
        for byte in bytes {
            if let Heard::Message(message) = lines.push(*byte) {
                crate::taken();
                std::hint::black_box(message.len());
            }
        }
    })
}

fn adverts() -> Vec<Vec<u8>> {
    let payload = payload(5).to_vec();
    let mut advert = vec![payload.len() as u8 + 1, 0xff];
    advert.extend_from_slice(&payload);
    vec![payload, advert]
}

fn read_advert(bytes: &[u8]) {
    if node_of(bytes).or(node_in_advert(bytes)).is_some() {
        crate::taken();
    }
}

fn air_frames() -> Vec<Vec<u8>> {
    let seeds = encoded_messages();
    seeds.iter().map(|body| olc_link_espnow::air::frame(&ME, &[3; 6], body)).collect()
}

fn read_air(bytes: &[u8]) {
    if olc_link_espnow::air::read(bytes, &ME).is_some() {
        crate::taken();
    }
}

fn headers() -> Vec<Vec<u8>> {
    let block = CoinConfig::olc().genesis();
    vec![block.header.encode().to_vec()]
}

fn read_header(bytes: &[u8]) {
    if let Some(header) = BlockHeader::decode(bytes) {
        crate::taken();
        std::hint::black_box(header.id());
    }
}

fn coins() -> Vec<Vec<u8>> {
    let mut stranger = CoinConfig::olc();
    stranger.name = "Other".into();
    stranger.rules.retarget_interval = 7;
    vec![CoinConfig::olc().to_bytes(), stranger.to_bytes()]
}

/// A coin heard from a stranger: its rules are only numbers until they are used.
fn read_coin(bytes: &[u8]) {
    let Some(mut coin) = CoinConfig::from_bytes(bytes) else {
        return;
    };
    crate::taken();
    coin.settle_floor();
    std::hint::black_box(coin.genesis().id());
    let rules = &coin.rules;
    for height in [0, 1, 1_000, 20_000, u64::MAX / 2, u64::MAX] {
        std::hint::black_box((rules.reward_at(height), rules.floor_at(height)));
    }
    std::hint::black_box(olc_core::Rules::floor_for(rules.block_time_ms));
}

fn hellos() -> Vec<Vec<u8>> {
    let all = crate::fixture::messages();
    let hello = all.iter().filter(|message| matches!(message, Message::Hello(_)));
    hello.map(Message::encode).collect()
}

fn read_hello(bytes: &[u8]) {
    if let Some(Message::Hello(hello)) = Message::decode(bytes) {
        crate::taken();
        let hello: Box<Hello> = hello;
        std::hint::black_box(hello.genesis());
    }
}

fn no_state(parse: fn(&[u8])) -> Parse {
    Box::new(parse)
}

pub fn targets() -> Vec<Target> {
    vec![
        Target { name: "network.read", seeds: messages_stamped, start: || no_state(read_network) },
        Target {
            name: "message.decode",
            seeds: encoded_messages,
            start: || no_state(decode_message),
        },
        Target { name: "frame.decoder", seeds: frames, start: start_decoder },
        Target { name: "fragment.reassembler", seeds: pieces, start: start_reassembler },
        Target { name: "line.lines", seeds: lines, start: start_lines },
        Target { name: "bluetooth.advert", seeds: adverts, start: || no_state(read_advert) },
        Target { name: "espnow.air", seeds: air_frames, start: || no_state(read_air) },
        Target { name: "header.decode", seeds: headers, start: || no_state(read_header) },
        Target { name: "coin.from_bytes", seeds: coins, start: || no_state(read_coin) },
        Target { name: "hello.decode", seeds: hellos, start: || no_state(read_hello) },
    ]
}
