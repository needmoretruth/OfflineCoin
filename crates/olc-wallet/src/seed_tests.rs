//! Against the BIP's own test vectors (python-mnemonic's vectors.json, passphrase "TREZOR").
use super::*;
use alloc::collections::BTreeSet;
use bip32::{DerivationPath, Prefix};

/// Entropy, words, seed and master key.
const VECTORS: [(&str, &str, &str, &str); 5] = [
    ("00000000000000000000000000000000", "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about", "c55257c360c07c72029aebc1b53c05ed0362ada38ead3e3e9efa3708e53495531f09a6987599d18264c1e1c92f2cf141630c7a3c4ab7c81b2f001698e7463b04", "xprv9s21ZrQH143K3h3fDYiay8mocZ3afhfULfb5GX8kCBdno77K4HiA15Tg23wpbeF1pLfs1c5SPmYHrEpTuuRhxMwvKDwqdKiGJS9XFKzUsAF"),
    ("ffffffffffffffffffffffffffffffff", "zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo wrong", "ac27495480225222079d7be181583751e86f571027b0497b5b5d11218e0a8a13332572917f0f8e5a589620c6f15b11c61dee327651a14c34e18231052e48c069", "xprv9s21ZrQH143K2V4oox4M8Zmhi2Fjx5XK4Lf7GKRvPSgydU3mjZuKGCTg7UPiBUD7ydVPvSLtg9hjp7MQTYsW67rZHAXeccqYqrsx8LcXnyd"),
    ("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff", "zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo vote", "dd48c104698c30cfe2b6142103248622fb7bb0ff692eebb00089b32d22484e1613912f0a5b694407be899ffd31ed3992c456cdf60f5d4564b8ba3f05a69890ad", "xprv9s21ZrQH143K2WFF16X85T2QCpndrGwx6GueB72Zf3AHwHJaknRXNF37ZmDrtHrrLSHvbuRejXcnYxoZKvRquTPyp2JiNG3XcjQyzSEgqCB"),
    ("7f7f7f7f7f7f7f7f7f7f7f7f7f7f7f7f7f7f7f7f7f7f7f7f", "실장 활동 큰절 흔적 형제 제대로 훈련 한글 실장 활동 큰절 흔적 형제 제대로 훈련 한글 실장 환갑", "3162bc17e0f2f01ee571022444d2c5fbddf6a68dedfe734c319fb574592e9c0328f6526116b3b0b025b23391781d0bef8f43bc8ddc2b054b9f52e1fd6a88e3d2", "xprv9s21ZrQH143K2cE8CZ9tqfnmvNZcEJpwnyRRcsqDPokupVqx1hecEjQF2zrM6amGPiDiEKGZdNWrps6DdEcP83UABBjV7yJSkAFUWcP1FDF"),
    ("f585c11aec520db57dd353c69554b21a89b20fb0650966fa0a9d6f74fd989d8f", "향상 담배 박수 추측 기술 충분히 협력 성적 줄무늬 인체 단위 딸아이 왼손 거짓 조깅 유명 석사 참석 이야기 크림 변동 진급 스케이트 하지만", "0ecef71bd6f0948d9186c2786086a00f7140a00d37c836d01567077aac0dbc69f62189c02a9138dcc79a74dbb676b74aad4959fdbbf1d06a7798385f8eec97b0", "xprv9s21ZrQH143K3zjSpKnVTgqBrJfMK5iZXLrGb5Z4zcWKo9URusFq4n7YjnLR3RCqD8Mytzk4qjHnk52PfdQUHCCJUnKnZxqhMXGGVVe1NXZ"),
];

fn bytes(text: &str) -> Vec<u8> {
    (0..text.len()).step_by(2).map(|at| u8::from_str_radix(&text[at..at + 2], 16).unwrap()).collect()
}

#[test]
fn both_lists_are_the_bips_own_2048_words() {
    for list in [WordList::English, WordList::Korean] {
        let words: BTreeSet<&str> = list.words().collect();
        assert_eq!(words.len(), 2048);
    }
}

#[test]
fn words_seed_and_master_key_match_the_vectors() {
    for (entropy, words, seed, master) in VECTORS {
        let list = if words.is_ascii() { WordList::English } else { WordList::Korean };
        let made = Mnemonic::from_entropy(&bytes(entropy), list).unwrap();
        assert_eq!(made.phrase(), words);
        let read = Mnemonic::parse(words).unwrap();
        assert_eq!(read.entropy(), bytes(entropy).as_slice());
        let made_seed = read.seed("TREZOR");
        assert_eq!(made_seed.as_slice(), bytes(seed).as_slice());
        let key = XPrv::new(made_seed).unwrap();
        assert_eq!(key.to_string(Prefix::XPRV).as_str(), master);
    }
}

#[test]
fn korean_words_read_in_decomposed_letters_and_with_extra_spaces() {
    let (_, words, _, _) = VECTORS[3];
    let decomposed: String = words.nfkd().collect();
    let spaced = decomposed.replace(' ', "   ");
    assert_eq!(Mnemonic::parse(&spaced).unwrap().phrase(), words);
}

#[test]
fn a_word_written_wrong_is_found_and_named() {
    let typo = "abandon abandon abandon abandom abandon abandon abandon abandon abandon abandon abandon about";
    assert_eq!(Mnemonic::parse(typo), Err(SeedError::Unknown(4)));
    let swapped = "about abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon";
    assert_eq!(Mnemonic::parse(swapped), Err(SeedError::Checksum));
    assert_eq!(Mnemonic::parse("abandon about"), Err(SeedError::Count(2)));
}

#[test]
fn keys_follow_the_olc_path() {
    let seed = Mnemonic::parse(VECTORS[0].1).unwrap().seed("");
    let path: DerivationPath = "m/44'/5196867'/0'/0/1".parse().unwrap();
    let expected = XPrv::derive_from_path(seed, &path).unwrap();
    let ours = derive_secret(&seed, 0, 1).unwrap();
    assert_eq!(ours, <[u8; 32]>::from(expected.private_key().to_bytes()));
    assert_ne!(derive_secret(&seed, 0, 0).unwrap(), ours);
    assert_ne!(derive_secret(&seed, 1, 1).unwrap(), ours);
}

#[test]
fn words_restore_the_same_address_and_every_key_with_coins() {
    let words = Mnemonic::from_entropy(&[42; 16], WordList::Korean).unwrap();
    let mut first = Wallet::from_words(&words, false).unwrap();
    for _ in 0..4 {
        first.next_word_key().unwrap().unwrap();
    }
    let with_coins = first.keys().nth(3).unwrap().0;

    // On another computer, from the words as a person types them back.
    let typed = Mnemonic::parse(&words.phrase()).unwrap();
    let mut second = Wallet::from_secret([77; 32]).unwrap();
    let added = second.restore_words(&typed, |address| *address == with_coins).unwrap();
    // Every key up to GAP past the last with coins, so coins a chain still on its way in holds
    // show once it arrives.
    let looked = 3 + GAP as usize + 1;
    assert_eq!(added, looked);
    assert_eq!(second.address(), first.address());
    assert_eq!(second.keys().nth(4).unwrap().0, with_coins);
    assert_eq!(second.key_count(), looked + 1);
    assert_eq!(second.seed.as_ref().unwrap().next, looked as u32);

    // An older wallet keeps its key and receives on the new words' first key.
    let mut older = Wallet::from_secret([78; 32]).unwrap();
    let old_address = older.address();
    older.add_words(&words).unwrap();
    assert_eq!(older.address(), first.address());
    assert_eq!(older.keys().next().unwrap().0, old_address);
}

#[test]
fn restoring_other_words_keeps_the_words_and_keys_already_here() {
    let a = Mnemonic::from_entropy(&[1; 16], WordList::English).unwrap();
    let b = Mnemonic::from_entropy(&[2; 16], WordList::English).unwrap();
    let mut wallet = Wallet::from_words(&a, true).unwrap();
    wallet.next_word_key().unwrap().unwrap();
    let a_keys: Vec<_> = wallet.keys().map(|(address, _)| address).collect();

    wallet.restore_words(&b, |_| false).unwrap();
    assert_eq!(wallet.seed.as_ref().and_then(SeedRecord::mnemonic).unwrap().phrase(), b.phrase());
    let kept: Vec<String> = wallet.earlier.iter().filter_map(SeedRecord::mnemonic).map(|words| words.phrase()).collect();
    assert_eq!(kept, [a.phrase()]);
    assert!(a_keys.iter().all(|address| wallet.owns(address)));
    assert_eq!(wallet.earlier[0].next, 2, "A's next key number stays too");

    // Both sets of words name their keys; restoring A again makes A the words once more.
    let named: BTreeSet<_> = wallet.word_addresses().into_iter().collect();
    assert!(a_keys.iter().all(|address| named.contains(address)));
    wallet.restore_words(&a, |_| false).unwrap();
    assert_eq!(wallet.seed.as_ref().unwrap().entropy, a.entropy());
    assert_eq!(wallet.earlier.len(), 1);
}

#[test]
fn keys_from_before_the_words_are_not_named_by_them() {
    let words = Mnemonic::from_entropy(&[3; 16], WordList::English).unwrap();
    let mut older = Wallet::from_secret([79; 32]).unwrap();
    let old_address = older.address();
    older.add_words(&words).unwrap();
    let named = older.word_addresses();
    assert!(!named.contains(&old_address));
    assert!(named.contains(&older.address()));
}
