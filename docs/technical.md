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

A pool on the computer gives a board on its cable work with `work`. A board heard only through another board gets the same work as a control packet signed with the app's control key: the board on the cable checks the signature and passes it one hop on by ESP-NOW, the board it is for checks it again and sends back what reached the difficulty the same way, and the pool credits that board's own address. Lent work ends a minute after the pool last renewed it. Both boards need build i19 or later. Each board answers at once: the board on the cable says it passed the work on, and the board it is for sends shares with none in them to say it took it. The pool sends unanswered work again every 5 seconds; after 15 seconds with no answer it switches the board off and names the silent board, since a board that does not trust the app's key drops the work without a word.

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

From the genesis of OLC and of every coin made since, a version 2 transaction carries what Bitcoin added after 2009. Its signatures name the coin, so a fork or another coin can't replay it. A signature may carry Bitcoin's kinds: ALL, NONE, SINGLE and ANYONECANPAY. Its id leaves out the unlock data, as SegWit's does, and the block's Merkle root still covers every byte. It has a lock time by height and sequences (BIP 68), and only it can spend gamma.

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

## Payment channels

Channels follow Lightning's design (BOLT 2, 3, 7 and 11) on OLC's own messages, so they do not talk to Bitcoin's Lightning nodes. Two computers lock coins in a 2-of-2 output of a version 2 transaction and pay each other by signing new balances. Each side's own share waits 144 blocks by default (2 to 2,016) when it closes alone, and a side that closes with a balance it has revoked loses the channel's coins to the other side. A payment to someone with no shared channel goes hop by hop with hash time locked contracts and shorter timeouts at each hop. The route travels in the clear, without onion routing. Channels are announced with both computers' keys and both funding keys, and a channel leaves the map when its funding output is spent. The node watches every block for revoked balances before it prunes anything. Desktop nodes run channels; boards check the chain.

```
channels
channel-open NODE AMOUNT [--delay BLOCKS]
channel-close CHANNEL [--force]
invoice AMOUNT [NOTE]
channel-pay REQUEST
```

## Changing the rules

A coin's rules can change at a height with no new coin. Expert mode has Change rules on the Network page, and `olc` has the same word. Blocks before that height stay valid, so the chain keeps its history and splits from nodes on the old rules, as a fork does in Bitcoin. You can go back.

## Blocks, difficulty and time

OLC started over on 3 November 2026 from a new genesis (time 1,793,664,000 seconds, message `OfflineCoin genesis 2026-11-03`), with today's rules from block 0: version 1 and version 2 transactions, the floor, the time rules and per-block difficulty. Coins made before keep the rules they were made with, and a coin file from before reads as it did.

### Header

A block of a coin made since carries Bitcoin's 80-byte header: version, previous block, Merkle root, time in seconds, bits and nonce, little-endian as in Bitcoin. Its id is the double SHA-256 of those bytes, and that hash, read as a little-endian number, must be at most the target in bits, as in Bitcoin Core's `CheckProofOfWork`. So a miner or a Stratum pool built for Bitcoin can work an OLC block. Coins made before keep OLC's first 88-byte header, with the time in milliseconds and the difficulty as a number, hashed as before.

- **Version**: BIP 9's top bits `001`. A Change rules fork marks its first block in bits 0 to 12, which no miner rolls (miners roll bits 13 to 28, BIP 310 and BIP 320). On coins made before, the mark is added to the difficulty; in the version it leaves the target to the difficulty rule, and a header alone still shows which side of a fork it is on.
- **Difficulty**: the hashes a block takes on average, which is Bitcoin Core's `GetBlockProof` of the target in bits. A difficulty rounds up to the nearest one a compact target stands for. OLC's floor, its proof-of-work limit, is 30,000 (bits `0x1f022f3d`): one device at 1,000 hashes a second finds a block every 30 seconds.
- **Coinbase**: its last output holds 8 zero bytes as data, Stratum's extranonce 1 and 2 of 4 bytes each. Stratum's `coinb1` is the coinbase up to them and `coinb2` what follows (`olc_core::coinbase_split`), and the coinbase's leaf is the double SHA-256 of the three together, as in Bitcoin. It is a counter, not a note: the list of notes leaves it out and the explorer shows it as hex.
- **Merkle tree**: Bitcoin's, pairs of double SHA-256 with the last repeated on an odd level. Each leaf is the hash of all of a transaction's bytes, as in Bitcoin before SegWit, so the root covers unlock data without a second tree in the coinbase.

### Difficulty rules

Each coin chooses one in its file as `rules.difficulty_rule`. A file from before the choice reads as `bitcoin`, which every coin had then. `rules.retarget_interval` of 0 keeps the difficulty fixed under every rule.

| Rule | What it does |
|---|---|
| `bitcoin` | Bitcoin Core's `pow.cpp`: every `retarget_interval` blocks the difficulty follows how long the last window took, at most four times either way, and never below the floor. OLC compares the window's N−1 gaps with N−1 block times. Bitcoin Core compares them with N, which is 0.05 % off at its 2,016 blocks and would be 10 % off at 10. |
| `bip94` | Bitcoin Core's test network since BIP 94. Bitcoin's rule, and between retargets a block more than two block times after its parent must carry the floor; any other carries the last difficulty that is not such a block's. A retarget starts from the first block of the window it measures, which never takes the exception. The first block of a window may be at most one block time before its parent (BIP 94's 600 seconds, scaled to the block time). |
| `asert` | Bitcoin Cash's aserti3-2d (spec `2020-11-15-asert.md`): at every block the difficulty halves for each `rules.half_life_blocks` block times the parent came late and doubles for each it came early. With `rules.allow_min_difficulty_blocks`, a block late enough must carry the floor, as below. |

OLC uses `asert` with a half-life of six blocks, 3 minutes, as Bitcoin Cash's test networks use one hour of their 10 minute blocks for hash that comes and goes. Counted in blocks, the half-life keeps its meaning when a coin chooses another block time.

ASERT alone sets a block's difficulty by when its parent came, so a miner who drops to a fiftieth of the hash rate waits about fifty block times for the next block. OLC also sets `allow_min_difficulty_blocks`, Bitcoin Core's `fPowAllowMinDifficultyBlocks` for its test networks: a block whose time is more than two block times and the clock limit after the newest time of the 11 blocks before it must carry the floor, 90 seconds on OLC. Such a block adds only the floor's work. ASERT anchors on the newest block before such blocks, as Bitcoin Cash's test networks keep their fixed anchor through them, so the regular difficulty goes on falling by the spec's formula over the time they took, and the first block in time carries it. A late block at the floor that ASERT itself brought there is stepped past the same way, which lands on the floor all the same, because every step past a late block only eases. ASERT looks back past at most 32 such blocks, inside the 64 headers a node that started from a peer's snapshot holds; past them the regular difficulty is the floor. When the first block itself comes late, the genesis anchors as if its parent were a block time before it, so the block after asks what it would have had the first block carried the starting difficulty.

Measured on 2026-11-04 with three real nodes on 127.0.0.1, one per rule, at OLC's numbers: 30 second blocks, windows of 10, the floor of 30,000. Each mined at 20,000 hashes a second until its difficulty had followed, then dropped to 1,000, where the floor gives 30 second blocks. "Back" is the first block whose own rule, not BIP 94's exception, asks a difficulty the low rate finds within 37.5 seconds.

| Rule | Difficulty at the drop | First block after it | Back | Then |
|---|---|---|---|---|
| `bitcoin` | 900,007 | 30.8 min | 25 blocks, 110.5 min | 26.5 s on average over 12 blocks |
| `bip94` | 708,202 | 1.0 min, then one every 75 s on average | 45 blocks, 56.3 min | 33.8 s over 103 blocks |
| `asert` | 567,592 | 11.5 min | 8 blocks, 16.3 min | 38 s over 160 blocks: after a fast block it rises, after a slow one the floor holds it |

ASERT here follows the spec in integer steps: the exponent `((Δt − T(Δh + 1)) × 2^16) / τ` divided toward zero, the split into whole shifts and a fraction, and the spec's cubic for 2 to the fraction. Bitcoin Cash moves a 256-bit target; OLC moves a difficulty, the target's inverse, so it runs the same steps with the exponent's sign turned, never below the floor. Each block anchors on its parent, which over blocks off the floor comes, up to rounding, to the spec's fixed anchor; a node needs only the grandparent, and a chain that sat at the floor through a quiet hour follows new hash power at its next block, where a fixed anchor would wait for the hour to be made up.

The spec's 12 published test vectors (`crates/olc-pow/vectors`) pass two ways. In the spec's own 256-bit form, with OLC's exponent and cubic, all 14,000 rows come out exactly, after times turn from seconds into milliseconds, which leaves every exponent as it was. Turned into difficulties, as 2^20 × the spec's limit / target, the 5,144 rows that fit in 64 bits land within 0.023 % of OLC's own step: the cubic's error each way and the 15 bits nBits keeps.

### Time rules

From OLC's new genesis, and on every coin made since:

- A block's time must be after the median of the 11 blocks before it (`rules.median_time_blocks`), as Bitcoin's median time past. With 30 second blocks that reaches back about 3 minutes: one miner with a wrong clock cannot hold the chain back, and a block's time stays near when it was found.
- A computer takes no block more than 30 seconds, one block time, ahead of its clock (`rules.max_ahead_ms`), and takes it when it comes again later, as Bitcoin Core does with 2 hours and Ethereum's miners did with 15 seconds at their 13. Clocks set over the internet agree within seconds, and under ASERT a block 30 seconds ahead makes the next one at most about 1.12 times easier. A board has no clock of the date, so it runs at the chain's time and sets no limit.
- The floor for a late block waits for the clock limit as well as two block times, and counts from the newest time of the 11 blocks before it. A miner that sets its clock ahead by the limit still waits two real block times, and a parent stamped back to the median time past opens it no sooner. On two block times alone, a node lying by the limit would take the floor straight after every block. A board, with no limit, has no such bound, as for every time rule.
- Under BIP 94, the first block of a window may be at most one block time before its parent.

Times are whole seconds on a coin with Bitcoin's header. The rules' numbers stay in milliseconds in the file. A coin made before keeps a block after its parent and no limit.

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

- A board takes a block of any time, because it has no clock of the date. A computer takes none more than 30 seconds ahead of its own.
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
