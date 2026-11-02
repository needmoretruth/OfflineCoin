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

## ESP32-S3

A board needs 16 MB of flash and 8 MB of PSRAM. An SH1106 OLED on I2C (SDA 8, SCL 9) is optional.

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
| Wi-Fi | Boards and computers | `network on` starts the board's own network, OLC-XXXX. `wifi SSID PASSWORD` joins a router |
| Bluetooth | Boards, and Linux computers | On by default |
| USB | A computer and its board | `olc node --serial PORT` |
| TCP | Computers | A home network, a VPN or the internet |

A computer on OLC-XXXX joins with `olc node --connect 192.168.4.1:7000`.

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
| `olc-lang` | OLCL alpha, beta, omega | Implement `LockLanguage` |
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
| `firmware` | The board: radios, flash, OLED | |

## Fonts

The Korean letters on the OLED come from [Galmuri](https://github.com/quiple/galmuri). OLC Core carries Inter, Noto Sans KR and IBM Plex Mono. All four are under the SIL Open Font License.

To remake the coin at the top of the README, see `tools/logo/spin_gif.py`.
