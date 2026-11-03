# Changelog

Notable changes for each version. How the numbers were taken is in [docs/measurements.md](docs/measurements.md).

## [Unreleased]

### Added

- Home tells a wallet that has no recovery words yet, such as one made before 0.2.0, that only the backup file brings it back, with a button that makes the words.
- OLC Core opens in your computer's language: Korean on a Korean system and English on any other, until you pick one in Settings.
- OLC starts over from a new first block with today's rules from block 0. Coins and blocks of the earlier OLC stay on that chain only.
- On the new OLC the difficulty follows each block: after a slow block the next is easier, and after a fast one harder. When no block comes for a minute and a half, the next may be mined at the lowest difficulty. A miner that drops to a fiftieth of its rate finds its next block about a minute and a half later, and blocks come about every 30 seconds again within about 25 minutes.
- Every coin chooses how its difficulty moves in its coin file: at each block, as above, or every set number of blocks as in Bitcoin, either as Bitcoin does it or as Bitcoin's test network does, where a block that comes after two block times may take the lowest difficulty. A coin file from before keeps Bitcoin's way.
- A block's time must be after the middle time of the 11 blocks before it, and a computer takes no block more than 30 seconds ahead of its clock, on the new OLC and every coin made since.
- New OLC blocks have Bitcoin's 80-byte header and proof of work, the shape mining software and machines made for Bitcoin work on, with room in the first payment for a pool's extra numbers.
- A mining pool on your computer that other computers join over Stratum v1, the way Bitcoin's pools work. This computer, the miners on it and your boards work in it, and when it finds a block each is paid for the shares it sent. OLC Core can also join a pool that runs on another computer.
- A board heard only by radio through the board on your cable works in the pool through that board. Its work goes out signed with the app's control key, and its shares come back the same way and are paid to it. Both boards need build i19 or later.
- A board that does not trust this app, or a cable board that does not pass the work on, is switched off after 15 seconds, and its row says why and what to do.
- Pool payouts show 2 decimals. Expert mode shows every decimal.
- Open a channel by pasting the other computer's channel address (key@host:port). Your own address shows in both modes with Copy and a QR code. Expert mode adds an amount to send at opening and a delay.
- `olc channel open KEY@HOST:PORT AMOUNT` opens a channel from the terminal the same way.
- Payment requests are listed with their state: waiting, paid or expired.
- Peers show in plain words in basic mode and as a table in expert mode: direction, ping, bytes sent and received, version and link. Disconnect a peer, or ban it for an hour or a day.
- `olc swap` and the other terminal words reach the OLC Core or `olc node` already running on the same data folder.
- History on Home and on the Wallet page shows the time, what happened, how sure it is and the amount, and moves with each block. A row opens its block with every coin in and out.
- Filter the history by kind and save it as CSV. Name a payment for yourself, pay several addresses at once, and ask for an amount with an `olc:` link.
- Blocks are drawn newest first with who found each, and blocks that lost a race are marked.
- The Lab runs a 51% attack on a practice coin on this computer, one step at a time.
- Mining can use every core but one. Set the speed on Home in steps of 25 %, 50 %, 75 % and max, or with a slider.
- Making a coin shows the three ways of moving difficulty side by side, measured on real nodes after mining falls 20 or 50 times or rises 50 times.

### Changed

- The menu reads Home, Mining, Wallet, Blocks, Payments, Lab, Coins and Devices, with Console and Settings below, in both modes.
- The pool moved from Devices to Mining.
- Settings come in four groups, General, Mining, Network and Display, with a button that returns them to their defaults.
- A computer combines small coins only after you turn it on, on the Wallet page.
- Explanations keep to three lines. Details opens the rest with a picture.

### Fixed

- `olc --version` and `olc-core --version` printed help instead of the version. They now print the program's name and version.
- The Move to the words button jumped up when the last mined coins matured, so a click at that moment missed it. The line about coins that must wait now sits below the button and keeps its place.
- Combining coins showed as a payment of 400 to yourself. It now reads as 8 coins joined into one, with an amount of 0.
- The wait for the next block counted only this computer's mining. It now counts every miner on the chain.
- Addresses added by hand were cleared while a chain opened, so those computers were not reached.
- On Windows a computer you connected to showed no address in the peer list, and a ban could miss it.

## [0.2.0] - 2026-11-03

### Added

- Today's Bitcoin transaction rules beside the first ones, from block 6,240 on OLC and from the first block on new coins: lock times, relative locks, a choice of what a signature covers, transaction ids that signatures cannot change, and Bitcoin's script words with up to 20 keys in a multisig. Payments in the first format stay valid, and nodes and miners take both.
- Signatures name their coin, so a payment made on one coin cannot be replayed on a fork of it. A fork made with Change rules takes only the new format from its fork height.
- Seed words. OLC Core shows 12 words when it makes a wallet, and a wallet comes back from them on any computer, from the first screen on. English and Korean word lists. Settings shows the words again after a warning.
- Coins a wallet received before it had words move to the words with one payment.
- Shared wallets for 2 to 20 people, with a set number of signatures needed to spend. Signatures pass between people as text or a QR code.
- Speed up. A payment still waiting goes out again with a higher fee. Miners also take a waiting payment together with a payment that spends it.
- Swap. Trade one coin for another with someone, with nobody in the middle. If the other side stops, your coins come back after the time you chose. `olc swap` does the same.
- Payment channels, after Lightning. Open a channel with another computer and pay at once, pay someone you share no channel with through computers in between, ask for money with a payment request as text or a QR code, and close together or alone. A side that closes with an old balance loses the channel's coins to the other side.
- Signed messages, in expert mode and with `olc sign-message` and `olc verify-message`.
- Sign the old way, in expert mode and with `olc version1 on`, for payments in the first format.

### Changed

- The balance counts change from your own waiting payments and leaves out coins a waiting payment already spends, so the balance and Send agree.
- A refused payment says why in plain words.
- QR codes are drawn large enough to scan from a screen.
- Notices with an amount show the coin's unit.

### Fixed

- Korean lines put a particle right after a number, such as 초당 20,139번로.

The first rules, close to Bitcoin in 2009 and 2010, stay on the [0.1.1 release](https://github.com/needmoretruth/OfflineCoin/releases/tag/v0.1.1).

## [0.1.1] - 2026-11-03

### Fixed

- Boards that joined the same Wi-Fi router could not find each other, and a computer had to be given their addresses. Boards now announce themselves on the router every 2 seconds, and boards and computers that hear them connect with nothing typed.
- A board stopped by a panic stayed stopped until someone pressed its reset button. It now restarts, and a new build that stops within its first minute sends the board back to the build it had before. OLC Core says so on the board's page.
- After an update, a board's page could show Restarting for minutes. It now shows the board's state as soon as the board answers.
- The default board brightness showed as Middle. It now shows as Dim.
- Text typed on one board's page, such as a Wi-Fi name, showed on another board's page.
- `olc node --help` started a node. `--help` and `-h` now print help after any command.

## [0.1.0] - 2026-11-03

### Added

- OLC Core, a desktop app for Windows, macOS and Linux. Basic mode uses plain words. Expert mode uses standard terms, adds finer settings and has a console.
- Builds for Linux on ARM, so OLC Core and `olc` run on a Raspberry Pi.
- Several nodes on one computer, each its own process, sized like a board or a Raspberry Pi.
- Pruned nodes, light nodes, watch-only addresses and signing on an offline computer.
- Links over jumper wires and Bluetooth. A Linux computer with the right Wi-Fi card can use ESP-NOW.
- One link for each neighbour, chosen from the links that are alive, in an order you can change.
- A pool and a 51% attack lab, both outside the coin and built from ordinary node operations.
- Radio updates for boards with no data cable. OLC Core first tries the build on a board with a cable, once by cable and once by radio.
- Installers: a setup program for Windows, disk images for macOS on Apple silicon and on Intel, and .deb packages for Linux on x86-64 and ARM.
- A board that gets hot mines at 100 hashes a second until it cools. OLC Core says why on the board's page and in the Activity column.
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
- An address added by hand was never dialed, so a node on the same computer could not be reached. It is now dialed at start, when the coin changes and when it is added.
- `olc keys` and `olc lock-sign` read different wallets. Every `olc` command now uses the OLC Core wallet unless `--data` names another folder.
- `olc node` lacked `lock-pay`, `lock-spend`, `lock-sign`, `scan`, `new-coin` and `save-coin`.
- `olc update --firmware FILE` ignored the file.
- `olc explore`, failed payments, some expert-mode notices and refused board orders showed internal names such as `KnownKey`. They now read as sentences.
