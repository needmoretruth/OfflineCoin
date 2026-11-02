#!/usr/bin/env python3
"""Payments between a miner, a pruned board and a light board, seen by a full node: kinds_of_node.py MINER PRUNED LIGHT KEEP [FULL_NODE_URL]."""
import json
import sys
import time
import urllib.request

import boards


def full_node(url):
    with urllib.request.urlopen(url + "/api/status", timeout=5) as reply:
        return json.load(reply)


def pay(sender, to, amount, label):
    print(label, boards.line(sender, f"pay {to} {amount}"), flush=True)


def settled(board, before, label):
    took = boards.wait_for(lambda: float(boards.status(board)["balance"]) != before, 400, 3)
    print(label, "balance moved after", took, "seconds:", boards.status(board)["balance"], flush=True)


def main():
    miner, pruned, light, keep = sys.argv[1:5]
    url = sys.argv[5] if len(sys.argv) > 5 else None
    print(boards.line(pruned, f"keep {keep}"), flush=True)
    print(boards.line(light, "light on"), flush=True)
    time.sleep(25)
    for board in (miner, pruned, light):
        fields = boards.status(board)
        print({key: fields.get(key) for key in ("height", "tip", "oldest", "held", "keep", "mining", "light", "balance", "unspent")}, flush=True)
    to_light = boards.status(light)["address"]
    to_pruned = boards.status(pruned)["address"]
    before = float(boards.status(light)["balance"])
    pay(miner, to_light, 5, "miner to light:")
    settled(light, before, "light")
    before = float(boards.status(pruned)["balance"])
    pay(light, to_pruned, 2, "light to pruned:")
    settled(pruned, before, "pruned")
    if url:
        node = full_node(url)
        before = float(boards.status(miner)["balance"])
        pay(pruned, node["address"], 1, "pruned to full node:")
        balance = node["balance"]
        took = boards.wait_for(lambda: full_node(url)["balance"] != balance, 400, 3)
        print("full node balance moved after", took, "seconds:", full_node(url)["balance"], flush=True)
    tips = {boards.status(board).get("tip") for board in (miner, pruned, light)}
    print("tips", tips, flush=True)
    if url:
        print("full node tip", full_node(url)["tip"], full_node(url)["height"], flush=True)


if __name__ == "__main__":
    main()
