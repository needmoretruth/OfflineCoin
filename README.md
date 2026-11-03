<p align="center">
  <img src="docs/images/coin.gif" width="340" alt="A spinning OfflineCoin drawn in text">
</p>

<h1 align="center">OfflineCoin</h1>

<p align="center">
  <strong>The niches are full, so let's go after the market that doesn't exist.</strong>
</p>

<p align="center">
  <a href="README.ko.md">한국어</a>
</p>

OfflineCoin is a cryptocurrency that works with no internet. You mine it, check it and send it yourself.

## Download

| Your computer | Download |
|---|---|
| Windows | [OLC Core for Windows](https://github.com/needmoretruth/OfflineCoin/releases/latest/download/OLC-Core-windows-x86_64-setup.exe) |
| Mac with an Apple chip | [OLC Core for Mac](https://github.com/needmoretruth/OfflineCoin/releases/latest/download/OLC-Core-macos-apple-silicon.dmg) |
| Mac with an Intel chip | [OLC Core for Intel Mac](https://github.com/needmoretruth/OfflineCoin/releases/latest/download/OLC-Core-macos-intel.dmg) |
| Ubuntu, Debian or Mint | [OLC Core for Linux](https://github.com/needmoretruth/OfflineCoin/releases/latest/download/OLC-Core-linux-x86_64.deb) |
| Raspberry Pi | [OLC Core for Raspberry Pi](https://github.com/needmoretruth/OfflineCoin/releases/latest/download/OLC-Core-linux-arm64.deb) |

Other systems are on the [releases page](https://github.com/needmoretruth/OfflineCoin/releases/latest).

Your computer may warn you the first time you open it.

- **Windows** says "Windows protected your PC". Choose **More info**, then **Run anyway**.
- **Mac** says it can't check the app. Open **System Settings**, then **Privacy & Security**, and choose **Open Anyway**.

## Get started

1. Open **OLC Core** and answer the short questions.
2. Press **Start mining**. Your computer starts looking for coins.
3. When it finds a block, coins appear on the Home page.

To get coins from someone, press **Receive** and give them your address. To pay someone, press **Send**.

## With other people

Pick **Two computers** when OLC Core asks what you'll mine with. If your computer asks whether the app may use the network, say yes.

Anyone on the same Wi-Fi opens OLC Core, goes to **Coins** and presses **Join**. No internet needed.

You can make your own coin too. On the **Coins** page, press **New coin**, then give it a name and your own rules.

- **Swap** trades your coin for someone else's coin, with nobody in the middle.
- **Channels** pay at once, even to people you share no channel with, as Lightning does for Bitcoin.
- **Shared wallets** under **Settings** need signatures from several people before coins leave it.

## ESP32 boards

There's a separate version of OfflineCoin for ESP32 boards. Once it's set up, a board needs no computer: it mines, keeps the chain and talks to other boards by radio. Plug one into your computer and press **Set up** in OLC Core.

## Keep your coins safe

- OLC Core shows 12 words when it makes your wallet. Write them on paper. With those words your wallet comes back on any computer, and without them or a backup your coins are gone.
- Anyone who sees your words or gets your wallet file can spend your coins, so keep them to yourself.
- A wallet made before version 0.2.0 has no words yet. Go to **Settings**, **Seed words** to make them and move your coins to them.

## A word from the developer (more than a word)

Hi, I'm NeedMoreTruth. I hope my name gets famous one day. (In a good way)

I'm a nerd who really, really loves cryptocurrency. The technology, the ideas, the design, all of it. I've tried and read up on a lot of coins, but I don't have much money, they aren't easy to buy, and it was all a bit awkward.

So I thought about running a real coin on my own computer. Cosmos SDK wasn't for me. Bitcoin Core's built-in test coins did too little. Nothing else out there satisfied me either. I'd already done most of what I could, and with no money and not much skill, I put together something simple: a coin you can make and try yourself, offline on your own network, very easily.

It's nothing grand, but here's what matters. I don't like fake simulations, and I don't like things that are hard to run.

So what do I want?

- The code can differ, but the core ideas, design, structure and features are exactly those of a real cryptocurrency.
- Nothing is fake. Real devices really compute, talk to each other, mine and verify.
- You use it through a simple, clear app, with no complicated commands.

If people take an interest in OfflineCoin later, I'd like to support it properly and build a site where you can try it right there. But I don't know yet if it's worth spending money on. I already have plenty of projects that will cost a lot, so I'll wait and see.

(Spending money here would be for fun. I have no plans to make money from this. Normally I'd just spend it here, but right now I'm building something really important and the money has to go there. Then again, if this project gets attention, even that money won't beat my inner nerd)

## The coin

| | |
|---|---|
| Name | OfflineCoin (OLC) |
| Chain | OLC Chain (OLCC) |
| Contracts | OLC Language (OLCL) |
| Mining | Proof of work, SHA-256d |
| Block time | 30 seconds |
| Block reward | 50 OLC, halving every 210,000 blocks |
| Total supply | About 21 million OLC |
| Premine | None |
| Like Bitcoin | Recovery words, multisig, time locks, speeding up a payment, coin swaps, payment channels |

## More

- [Technical guide](docs/technical.md): the command line, boards, connections and how to build it.
- [Measurements](docs/measurements.md): what it does on real hardware.
- [Changelog](CHANGELOG.md): what changed in each version.
- [Version 0.1.1](https://github.com/needmoretruth/OfflineCoin/releases/tag/v0.1.1): the first rules, close to Bitcoin in 2009 and 2010.

## License

Apache-2.0. Pull requests need one line agreeing to the [CLA](CLA.md).
