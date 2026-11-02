#!/usr/bin/env python3
"""Compares Bluetooth on and off on every board, in turns, with blocks, branch moves, link moves and rate: bluetooth_compare.py MINUTES ROUNDS [LOG]."""
import re
import sys
import time

import boards

EVERY = 5


def set_all(ids, state):
    for board in ids:
        boards.line(board, f"bluetooth {state}")
    time.sleep(25)


def boots(board):
    return boards.line(board, "boots") or ""


def turn(ids, state, minutes, log):
    set_all(ids, state)
    first = {board: boards.status(board) for board in ids}
    rates = {board: [] for board in ids}
    behind = {board: 0 for board in ids}
    moved = {board: 0 for board in ids}
    reorg = {board: first[board].get("reorg") for board in ids}
    fell_back = {board: 0 for board in ids}
    lost = {board: 0 for board in ids}
    gone = 0
    samples = 0
    start = time.time()
    while time.time() - start < minutes * 60:
        now = {}
        for board in ids:
            try:
                now[board] = boards.status(board)
            except OSError:
                gone += 1
        if len(now) == len(ids) and all("height" in fields for fields in now.values()):
            top = max(int(fields["height"]) for fields in now.values())
            samples += 1
            for board, fields in now.items():
                rates[board].append(int(fields["hashrate"]))
                behind[board] += int(fields["height"]) < top - 1
                lost[board] += int(fields["peers"]) < 1
                fell_back[board] += fields["reorg"] != reorg[board]
                reorg[board] = fields["reorg"]
        else:
            gone += 1
        log.write(f"{time.time():.0f} {state} " + " ".join(
            f"{fields.get('height')}/{fields.get('hashrate')}/{fields.get('peers')}/{fields.get('reorg')}"
            for fields in now.values()) + "\n")
        log.flush()
        time.sleep(EVERY)
    last = {board: boards.status(board) for board in ids}
    for board in ids:
        moved[board] = sum(int(count) for count in re.findall(r"moves=(\d+)", boards.line(board, "near") or ""))
        print(f"{state} {board[-4:]} blocks +{int(last[board]['height']) - int(first[board]['height'])} "
              f"hash {sum(rates[board]) / max(1, len(rates[board])):.1f} "
              f"behind {behind[board]}/{samples} no_peer {lost[board]} "
              f"reorgs {fell_back[board]} link_moves {moved[board]} reset {last[board]['reset']} "
              f"temp {last[board]['temp']} rssi {last[board]['rssi']} no_answer {gone}", flush=True)


def main():
    minutes, rounds = float(sys.argv[1]), int(sys.argv[2])
    log = open(sys.argv[3] if len(sys.argv) > 3 else "bluetooth.log", "a", encoding="ascii")
    ids = [board["id"] for board in boards.boards()]
    before = {board: boots(board) for board in ids}
    for _ in range(rounds):
        turn(ids, "on", minutes, log)
        turn(ids, "off", minutes, log)
    for board in ids:
        print(board[-4:], "boots before:", before[board], "| after:", boots(board), flush=True)


if __name__ == "__main__":
    main()
