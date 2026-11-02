# Measurements

These numbers come from real hardware, taken on 2026-11-01 and 2026-11-02 on the builds leading up to 0.1.0, at default settings on the OLC chain unless a row says otherwise. The board experiments can be repeated with the scripts in [tools/measure](../tools/measure). The top of each script says how.

## Setup

- Three ESP32-S3-WROOM-1 N16R8 boards with SH1106 panels. Two are on USB data cables and joined by jumpers on GPIO 17, GPIO 18 and ground. One is on a power-only cable.
- A Linux desktop with an Intel AX200 card as a fourth node.
- Board defaults: 80 MHz, hash cap 1,000, screen at 10 frames a second, Bluetooth on, the board's own Wi-Fi off.
- Bluetooth became a default last, so most rows ran with it off. The last table shows what it changes.
- Temperatures come from the chip's sensor, in one room, over those two days.

## Mining

| Setting | Hash rate |
|---|---|
| 80 MHz, cap 1,000 (default) | 997 to 1,000 H/s |
| 80 MHz, no cap | 3,473 to 4,446 H/s |
| `full on`: top clock, no cap | 9,452 to 12,453 H/s |

The rate at cap 1,000 stayed at 997 to 1,000 H/s at height 30,200. The ranges cover three boards.

| Clock and cap | Chip temperature |
|---|---|
| 80 MHz, cap 1,000 | 28.2 to 30.4 °C |
| 240 MHz, cap 3,000 | 44.7 °C |
| 240 MHz, cap 5,000 | 45.0 °C |
| 240 MHz, cap 10,000 | 46.6 °C |
| 240 MHz, no cap | 49.1 °C |

The 240 MHz rows drew the screen at 30 frames a second.

## A board's chain

| What | Result |
|---|---|
| Boot on the OLC chain, height about 3,400 | 1.5 to 2.0 s |
| Boot on a test coin, height 30,159 | 2.8 s |
| Free memory at height 30,159, 4,030 unspent coins | 7.27 MB |
| Free memory at height 3,429, 730 unspent coins | 8.05 MB |
| Memory for each unspent coin | 237 bytes |
| Flash holding 16,010 blocks after pruning | 6.16 MB |
| Return after a forced watchdog reset | 20 of 20 |
| Light mode, memory for each header | 141 bytes |

## Computers

| What | Result |
|---|---|
| Sync 35,000 blocks | 313 s |
| Memory for each block | 3.42 bytes |
| Light node, sync 35,001 headers | 32 s |
| Light node, memory for each header | about 110 bytes |
| Node on the desktop | 7 to 10 MB of memory, about 1% of a core |

## Links

Sync between the Linux desktop and a board:

| Link | Blocks | Time | Blocks a second |
|---|---|---|---|
| Wi-Fi | 3,628 | 75 s | up to 68 |
| Bluetooth | 3,644 | 6 min 10 s or less | 9.8 or more |
| ESP-NOW | 3,947 | 7 min | 9.4 |

Payments confirmed in both directions on each of the three.

Corrupted frames over jumpers in 30 seconds, in each direction:

| Baud | Frames |
|---|---|
| 921,600 (default) | 0 and 0 |
| 1,500,000 | 7 and 1 |
| 2,000,000 | 45 and 28 |

When a link stops and another is ready:

| What | Result |
|---|---|
| The board whose link stopped moves to the next link | 0.01 to 0.7 s |
| The board on the other end moves | 4.2 to 9.3 s |
| Return to ESP-NOW after it comes back | 30.4 to 30.9 s |
| Redials while connected, desktop, 30 minutes | 0 |

Median block propagation between boards is 203 ms, with 1,321 ms the slowest seen, wired link first.

## Radio updates

The firmware image is 1.94 MB.

| Route | Time |
|---|---|
| ESP-NOW first, Bluetooth off | 67 s |
| ESP-NOW first, Bluetooth on | 97 s |
| Wired link first | 102 s |

## Experiments

| Experiment | Result |
|---|---|
| Two boards with 66.7% of the hash rate | A 25-block reorganization undid a payment |
| Network split for 12 minutes | Rejoined within seconds and dropped 6 blocks |
| A board leaves | Difficulty fell from 116,830 to 36,553, by at most a factor of 4 in each 10-block period |
| One node on other rules, with a longer chain | Boards on the OLC rules did not move: 0 reorganizations |
| Full, pruned, light and mining nodes together | 3 payments arrived, same tip everywhere |
| Fresh board fed by boards, no computer | About 30 blocks a second early on, 0.4 through blocks of merged coins at 80 MHz, about 11 minutes in all at `full on` |
| Payment relayed by boards, no computer | Accepted, and its block arrived 141 s later |
| Line of three boards, forced with the ignore list | Payment reached the far board in 0.9 s, block came back in 0.2 s |
| Settings after a restart | Cap, brightness, `full on` and `off` all kept |
| Radio at 5 dBm, board on a USB hub, 84 minutes | 0 brown-outs, signal 13 to 19 dB weaker than at 20 dBm |

Bluetooth on and off, three boards, four rounds of 21 minutes:

| | Bluetooth on | Bluetooth off |
|---|---|---|
| Hash rate | 986.3 H/s | 989.8 H/s |
| Disconnects, restarts, chain splits | 0 | 0 |
| Stale blocks | 1 in 79 | 0 in 70 |
