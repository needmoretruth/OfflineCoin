# Changelog

Notable changes for each version. How the numbers were taken is in [MEASUREMENTS.md](MEASUREMENTS.md).

## [Unreleased]

### Added

- OLC Core, a desktop app for Windows, macOS and Linux. Basic mode uses plain words. Expert mode uses standard terms, adds finer settings and has a console.
- Builds for Linux on ARM, so OLC Core and `olc` run on a Raspberry Pi.
- Several nodes on one computer, each its own process, sized like a board or a Raspberry Pi.
- Pruned nodes, light nodes, watch-only addresses and signing on an offline computer.
- Links over jumper wires and Bluetooth. A Linux computer with the right Wi-Fi card can use ESP-NOW.
- One link for each neighbour, chosen from the links that are alive, in an order you can change.
- A pool and a 51% attack lab, both outside the coin and built from ordinary node operations.
- Radio updates that reach boards with no data cable, once a board on a cable has proved the build.
- Scripts in `tools/measure` that repeat the board experiments in MEASUREMENTS.md.

### Changed

- A board starts in 2.8 s at height 30,159. It took 239 s at height 2,446.
- A radio update takes 67 s for a 1.94 MB image. It took 687 s for a 1.38 MB image.
- A node holds 3.42 bytes of memory for each block. It held 484.
- Boards default to 80 MHz, a cap of 1,000 hashes a second, the coin screen at 10 frames a second, low brightness and their own Wi-Fi off.
- Blocks come every 30 seconds. On OLC the lowest difficulty applies from height 20,000.

### Removed

- The browser screens. OLC Core replaces them.

### Fixed

- A peer that claimed more work and then went quiet could stop a node from mining.
- A connected node kept redialing: over 700 tries in four minutes, now none in 30 minutes.
- Printing took one byte out of the USB receive buffer each time, which corrupted command bursts.
- A board stalled for 8.3 s while signing a merge of small coins. The longest stall is now about 1.1 s.
- A board stalled for up to 2 s when serving old blocks.
