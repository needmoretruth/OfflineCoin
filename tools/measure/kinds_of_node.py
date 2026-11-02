#!/usr/bin/env python3
"""Payments between a miner, a pruned board and a light board, seen by a full node if asked: kinds_of_node.py MINER PRUNED LIGHT KEEP [COIN_FILE HOST:PORT].

With a coin file and the address of a board on the network, it starts a full node here with `olc start`.
"""
import json
import subprocess
import sys
import tempfile
import threading
import time

import boards


class FullNode:
    """A node of this computer, read from the status line it prints every second."""

    def __init__(self, coin, peer):
        folder = tempfile.mkdtemp(prefix="olc-full-node-")
        self.newest = {}
        self.process = subprocess.Popen(
            [boards.OLC, "start", "--data", folder, "--wallet", folder, "--coin", coin,
             "--port", "0", "--connect", peer],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
        threading.Thread(target=self.follow, daemon=True).start()
        if boards.wait_for(lambda: "address" in self.newest, 30, 1) is None:
            sys.exit("The full node did not start.")

    def follow(self):
        for said in self.process.stdout:
            self.newest = json.loads(said)

    def stop(self):
        self.process.stdin.close()
        self.process.wait(10)


def pay(sender, to, amount, label):
    print(label, boards.line(sender, f"pay {to} {amount}"), flush=True)


def settled(board, before, label):
    took = boards.wait_for(lambda: boards.status(board)["balance"] != before, 400, 3)
    print(label, "balance moved after", took, "seconds:", boards.status(board)["balance"], flush=True)


def main():
    boards.needs(4, __doc__)
    miner, pruned, light, keep = sys.argv[1:5]
    node = FullNode(sys.argv[5], sys.argv[6]) if len(sys.argv) > 6 else None
    print(boards.line(pruned, f"keep {keep}"), flush=True)
    print(boards.line(light, "light on"), flush=True)
    time.sleep(25)
    for board in (miner, pruned, light):
        fields = boards.status(board)
        print({key: fields.get(key) for key in ("height", "tip", "oldest", "held", "keep", "mining", "light", "balance", "unspent")}, flush=True)
    to_light = boards.status(light)["address"]
    to_pruned = boards.status(pruned)["address"]
    before = boards.status(light)["balance"]
    pay(miner, to_light, 5, "miner to light:")
    settled(light, before, "light")
    before = boards.status(pruned)["balance"]
    pay(light, to_pruned, 2, "light to pruned:")
    settled(pruned, before, "pruned")
    if node:
        balance = node.newest["balance"]
        pay(pruned, node.newest["address"], 1, "pruned to full node:")
        took = boards.wait_for(lambda: node.newest["balance"] != balance, 400, 3)
        print("full node balance moved after", took, "seconds:", node.newest["balance"], flush=True)
    tips = {boards.status(board).get("tip") for board in (miner, pruned, light)}
    print("tips", tips, flush=True)
    if node:
        print("full node tip", node.newest["tip"], node.newest["height"], flush=True)
        node.stop()


if __name__ == "__main__":
    main()
