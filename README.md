# OfflineCoin

```
             ################
          +#####===========#####
       *####===================####
     #######=====================####
    -###==####==#############======###
   =###=====####===========######===###
  ####==###===####=============####==###
  :##==##=======####==######======##==##
 **##=======###===####===######=======###
 =###======###======####=====###======###
 --##============##===####============###
  +##==========#####===#####==========##
  :###===================#####=======###
   =###=============###====####=====###
    =###===========#####=====####==###
     ..###==========###========######
       :####===================####
          :#####===========#####
             .###############
```

A Bitcoin-style coin for networks with no internet. It runs on computers and on ESP32-S3 boards.

- **OLC**: OfflineCoin, the coin.
- **OLCC**: OLC Chain, the blockchain.
- **OLCL**: OLC Language, the contract languages. Alpha pays an address, beta is a stack language like early Bitcoin Script, and omega is reserved for later.

Everything is a module. Swap one out or add your own and use it however you like. Keys are stored unencrypted and anyone with more hash power can rewrite the chain, so keep it a toy.

## Computer

```
cargo run --release -p olc-cli -- devnet
```

Three nodes mine on one machine. Their screens are at http://127.0.0.1:8100, 8101 and 8102, in English and Korean.

```
olc node --connect HOST:7000          join another node
olc node --serial /dev/ttyACM0        join through a board on USB
olc node --mine --hash-limit 50000    cap this node's hash rate
olc new-coin my.json --name MyCoin --ticker MINE --block-time-ms 5000
olc node --coin my.json
olc logo
```

A coin file is JSON. Change the rules on some nodes and they fork off the same chain. Change the name or the genesis message for a new chain.

## ESP32-S3

You need 16 MB of flash and 8 MB of PSRAM, plus [espup](https://github.com/esp-rs/espup) and [espflash](https://github.com/esp-rs/espflash). An SH1106 OLED is optional: SDA on GPIO 8, SCL on GPIO 9.

```
cd firmware
. ~/export-esp.sh
cargo run --release
```

A board mines and checks every block by itself and keeps its chain in flash. Boards find each other over ESP-NOW. Each one also hosts an open Wi-Fi network named OLC-XXXX, so a computer or phone can join it and run `olc node --connect 192.168.4.1:7000`.

The BOOT button changes the screen between basic, detail and easy. Hold it to switch between English and Korean.

Commands over USB serial: `status`, `screen`, `logo`, `view basic|detail|easy`, `lang en|ko`, `pay ADDRESS AMOUNT`, `mine on|off`, `limit HASHES`, `offline on|off`, `espnow on|off`, `wifi SSID [PASSWORD]`, `wifi off`, `dial IP:PORT`, `dial off`.

If a board's screen stays dark after powering up, press RST once: some boards stop in download mode.

## Modules

| Crate | What it is | To replace it |
|---|---|---|
| `olc-core` | Blocks, transactions, rules, the chain | |
| `olc-crypto` | SHA-256d and secp256k1 | Edit this one crate |
| `olc-pow` | Proof of work | Implement `Consensus` |
| `olc-lang` | OLCL alpha, beta, omega | Implement `LockLanguage` |
| `olc-wallet` | Keys and payments | |
| `olc-node` | Mempool, gossip, mining | |
| `olc-link-tcp`, `olc-link-serial` | Transports | Implement `Link` |
| `olc-logo` | The spinning coin | |
| `olc-cli` | The `olc` program and its screen | |
| `firmware` | The board: ESP-NOW, Wi-Fi, USB, flash, OLED | |

## License

Apache-2.0. Pull requests need one line agreeing to the [CLA](CLA.md). The Korean letters on the OLED come from [Galmuri](https://github.com/quiple/galmuri) under the SIL Open Font License.
