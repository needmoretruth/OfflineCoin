# Changelog

Notable changes for each version. How the numbers were taken is in [docs/measurements.md](docs/measurements.md).

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
- Installers: a setup program for Windows, disk images for macOS on Apple silicon and on Intel, and .deb packages for Linux on x86-64 and ARM.
- A board that gets hot mines at 100 hashes a second until it cools, and its page in OLC Core says why.
- A board screen set brighter than the default dims back after an hour with nobody touching the board. A board with no screen keeps running, and a board whose program stops restarts on its own.
- A board on a coin's earlier rules shows as other rules in OLC Core. It can move to the app's rules and keeps its wallet and its blocks up to where the rules differ.
- Home says when nobody nearby has the old blocks, and what to do about it.
- Scripts in `tools/measure` that repeat the board experiments in docs/measurements.md.
- A note from the developer, in the README and under Settings.

### Changed

- A board starts in 2.8 s at height 30,159. It took 239 s at height 2,446.
- A radio update takes 67 s for a 1.94 MB image. It took 687 s for a 1.38 MB image.
- A node holds 3.63 bytes of memory for each block. It held 484.
- Boards default to 80 MHz, a cap of 1,000 hashes a second, the coin screen at 10 frames a second, low brightness and their own Wi-Fi off.
- Blocks come every 30 seconds. On OLC the lowest difficulty applies from height 20,000.
- A transaction relayed from a stranger may hold at most 20 signature checks. One check takes up to 115 ms on a board at 80 MHz, so 20 cannot stall a board's links.

### Removed

- The browser screens. OLC Core replaces them.

### Fixed

- Random or damaged bytes from any link could crash a node or a board. Thirty million such inputs across 30 parsers now cause no panic.
- A stranger could fill a node's memory or its connections. Each now has a limit.
- A read error on the wallet file could lead to a new key being written over it.
- A peer that claimed more work and then went quiet could stop a node from mining.
- A connected node kept redialing: over 700 tries in four minutes, now none in 30 minutes.
- Printing took one byte out of the USB receive buffer each time, which corrupted command bursts.
- A board stalled for 8.3 s while signing a merge of small coins. The longest stall is now about 1.1 s.
- A board stalled for up to 2 s when serving old blocks.
- Turning off "Let others on this network connect" left the network port open until the app restarted. It now closes at once and drops the computers already connected.
- Settings, keys and payments made while a chain was still loading could be lost.
- A node whose usual ports were all taken ran without one and dropped out of the node list. It now takes any free port.
- The window could open larger than a small screen. It now opens at 90% of the screen at most, centred.
- If OLC Core could not start, nothing appeared. It now shows the reason in a system message box.
- Coins moved between your own addresses showed as a payment of 0 OLC. They now show as "To yourself".
- A board moved late onto other rules kept blocks from both sides and never split from the coin it left.
- `ignore` silenced a board only on ESP-NOW. It now covers every link.
- A long board name stopped mid-word on the board screen. It now ends in two dots.
