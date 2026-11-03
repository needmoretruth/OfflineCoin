# Technical guide

For people who want the details. To simply use OfflineCoin, the [README](../README.md) is enough.

## Command line

`olc` with no words lists everything it does.

```
olc node --mine                       run a node and mine
olc node --connect HOST:7000          join another node
olc node --serial /dev/ttyACM0        join through a board on USB
olc node --mine --hash-limit 50000    cap this node's hash rate
olc devnet                            three nodes on one machine
olc new-coin my.json --name MyCoin --ticker MINE --block-time-ms 5000
olc node --coin my.json
olc logo
```

A coin file is JSON. Change the rules on some nodes and they fork off the same chain. Change the name or the genesis message for a new chain.

## Build

```
cargo build --release -p olc-app -p olc-cli
```

`cargo run` and `cargo test` leave USB alone so that a test never touches a board. Set `OLC_NO_USB=0` to reach boards from a source run.

## The ESP32 version

OfflineCoin is a desktop program first. The ESP32 version in `firmware` is a separate program that follows the same rules. It runs on an ESP32-S3 with 16 MB of flash and 8 MB of PSRAM. An SH1106 OLED on I2C (SDA 8, SCL 9) is optional.

Plug the board in and press Set up in OLC Core. To flash from source, install [espup](https://github.com/esp-rs/espup) and [espflash](https://github.com/esp-rs/espflash):

```
cd firmware
. ~/export-esp.sh
cargo run --release
```

A board mines and checks every block by itself and keeps its chain in flash. It starts at 80 MHz, 1,000 hashes a second, the coin screen at 10 frames a second and low brightness, and it moves the picture every few minutes to spare the panel. `full on` gives it the top clock and no cap.

The BOOT button changes the screen between basic, detail and easy. Hold it to switch between English and Korean.

Commands over USB serial: `status`, `screen`, `boots`, `logo`, `view`, `lang`, `mine`, `limit`, `smooth`, `power`, `bright`, `ignore`, `offline`, `espnow`, `bluetooth`, `links`, `length`, `near`, `wires`, `pay`, `pool`, `tx`, `trust`, `name`, `clock`, `full`, `network`, `wifi`, `dial`, `keys`, `key`, `mineto`, `keep`, `light`, `merge`, `note`, `minenote`, `snapshot`, `coin`, `work`.

If a board's screen stays dark after powering up, press RST once: some boards stop in download mode.

## Connections

A node listens on every link it has turned on and keeps a table of its neighbours. Each neighbour gets the first living link in your order, and the node moves back when a better link has stayed up for a while. The order starts as wired, ESP-NOW, Wi-Fi, Bluetooth.

| Link | Reaches | How |
|---|---|---|
| Wired | Boards | GPIO 17, GPIO 18 and ground. Straight or crossed, the boards work it out |
| ESP-NOW | Boards, and Linux with a Wi-Fi card in monitor mode | On by default |
| Wi-Fi | Boards and computers | `network on` starts the board's own network, OLC-XXXX. `wifi SSID PASSWORD` joins a router, where boards broadcast their id on UDP 7000 every 2 s and dial the boards they hear. A computer's node on that network hears them too and dials them, once its firewall lets UDP 7000 in there |
| Bluetooth | Boards, and Linux computers | On by default |
| USB | A computer and its board | `olc node --serial PORT` |
| TCP | Computers | A home network, a VPN or the internet |

A computer on OLC-XXXX joins with `olc node --connect 192.168.4.1:7000`.

## Contracts

OLCL alpha pays an address. OLCL beta is a stack language at the level of early Bitcoin Script, written as words such as `HASH 0x... EQUAL`. OLCL gamma is Bitcoin Script as it runs today without Taproot, with Bitcoin's word names and numbers: CHECKSEQUENCEVERIFY, CHECKMULTISIGVERIFY with up to 20 keys, SHA256, HASH160, the stack and number words. `HASH` stays OLC's address hash and `CHECKHEIGHT` compares with the block height. Expert mode has a Locks page with worked examples of a hash lock, a two-of-two signature and a time lock, and `olc` has the same words. OLCL omega is reserved for later.

## Transaction versions

From height 6,240 on OLC, and from the genesis of a coin made since, a version 2 transaction carries what Bitcoin added after 2009. Its signatures name the coin, so a fork or another coin can't replay it. A signature may carry Bitcoin's kinds: ALL, NONE, SINGLE and ANYONECANPAY. Its id leaves out the unlock data, as SegWit's does, and the block's Merkle root still covers every byte. It has a lock time by height and sequences (BIP 68), and only it can spend gamma.

Version 1 transactions stay valid beside them in the same blocks, as legacy transactions do in Bitcoin. Wallets write version 2. Expert mode's "Sign the old way" and `olc version1 on` write version 1. A coin made with Change rules takes only version 2 from its fork height.

## Wallet

A wallet made by OLC Core 0.2.0 comes from 12 or 24 BIP 39 words, English or Korean, and derives its keys with BIP 32 on the path m/44'/5196867'/0'/0/i. A wallet made earlier keeps its keys and can add words; `words move` sends its older coins to the words in one payment. Wallets several people share hold 2 to 20 keys and spend with a set number of signatures, passed as text or a QR code. A waiting payment can be replaced with one paying a higher fee rate and a higher total fee, as full replace-by-fee works in Bitcoin Core 28, and block templates pick payments by the fee rate of a payment together with the waiting payments it spends. Signed messages work like Bitcoin's signmessage with an OLC prefix.

```
words [written|new [korean]|restore WORD...|move]
sign-message [--address ADDRESS] TEXT
verify-message ADDRESS SIGNATURE TEXT
shared [new N KEY...|add TEXT|forget N|pay N ADDRESS AMOUNT]
speed-up ID
version1 on|off
```

## Swaps

A swap trades two coins with hash time locked contracts, as atomic swaps do between Bitcoin and other chains. The offer names each coin by the hash of its coin file and carries the maker's address, and the taker's node fetches a coin it does not have from the maker and checks the hash. If the other side stops, each side takes its coins back after the wait it chose. The Swap page and `swap` do the same.

```
swap offer COIN GIVE TAKE [--wait MINUTES] | accept TEXT | answer TEXT | cancel ID | list
```

## Changing the rules

A coin's rules can change at a height with no new coin. Expert mode has Change rules on the Network page, and `olc` has the same word. Blocks before that height stay valid, so the chain keeps its history and splits from nodes on the old rules, as a fork does in Bitcoin. You can go back.

## Kinds of node

Boards and computers run the same rules, and each person picks what their node does.

- A full node checks and keeps every block.
- A pruned node checks every block and keeps the recent ones.
- A light node keeps headers and checks Merkle proofs, as in section 8 of the Bitcoin paper. It cannot mine.
- A full or pruned node mines or doesn't, as you like.

Watch-only addresses and signing on an offline computer work on any of them. A pool and a 51% attack lab sit outside the coin, built from ordinary node operations.

## Modules

| Crate | What it is | To replace it |
|---|---|---|
| `olc-core` | Blocks, transactions, rules, the chain | |
| `olc-crypto` | SHA-256d and secp256k1 | Edit this one crate |
| `olc-pow` | Proof of work | Implement `Consensus` |
| `olc-lang` | OLCL alpha, beta, gamma, omega | Implement `LockLanguage` |
| `olc-wallet` | Keys and payments | |
| `olc-node` | Mempool, gossip, mining | |
| `olc-store` | A node's files on a computer | |
| `olc-link-tcp`, `olc-link-serial`, `olc-link-ble`, `olc-link-espnow` | Transports for computers | Implement `Link` |
| `olc-control` | Signed orders to boards and radio updates | |
| `olc-board` | Reading and commanding boards from a computer | |
| `olc-service` | Everything the app does, with no screen | |
| `olc-platform` | What differs between operating systems | |
| `olc-app` | OLC Core, the window over `olc-service` | |
| `olc-cli` | The `olc` program over `olc-service` | |
| `olc-pool`, `olc-lab` | The pool and the experiments, outside the coin | |
| `olc-logo` | The spinning coin | |
| `olc-compat` | Checks that old and new builds agree on the chain | |
| `olc-fuzz` | Throws random and damaged bytes at every parser | |
| `firmware` | The ESP32 version: radios, flash, OLED | |

## Known limits

These come from the rules, and changing them means a fork.

- A block's time has no upper limit, because a board has no clock of its own.
- A block has no limit on signature checks. Nodes won't relay or mine a transaction with more than 20, so stalling a board takes a mined block.
- A version 1 transaction id covers its unlock data, as in Bitcoin before SegWit.
- A 64-byte transaction could be mistaken for an inner node of the Merkle tree.

These come from radio and storage.

- ESP-NOW frames are not encrypted, so someone nearby can fake a neighbour's address and cut it off for 10 minutes.
- A wrong piece during a radio update can spoil that update. It can't take the board over.
- Keys in a board's flash are not encrypted.

## Fonts

The Korean letters on the OLED come from [Galmuri](https://github.com/quiple/galmuri). OLC Core carries Inter, Noto Sans KR and IBM Plex Mono. All four are under the SIL Open Font License.

To remake the coin at the top of the README, see `tools/logo/spin_gif.py`.
