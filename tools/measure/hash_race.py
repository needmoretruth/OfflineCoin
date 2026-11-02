#!/usr/bin/env python3
"""Hash rate and blocks won per board over a stretch, with a line sent first: hash_race.py MINUTES [LINE]."""
import sys
import time

import boards


def main():
    boards.needs(1, __doc__)
    minutes = float(sys.argv[1])
    ids = [board["id"] for board in boards.boards()]
    if len(sys.argv) > 2:
        for board in ids:
            boards.line(board, sys.argv[2])
        time.sleep(25)
    first = {board: boards.status(board) for board in ids}
    rates = {board: [] for board in ids}
    missed = {board: 0 for board in ids}
    start = time.time()
    while time.time() - start < minutes * 60:
        for board in ids:
            value = boards.status(board).get("hashrate")
            if value:
                rates[board].append(int(value))
            else:
                missed[board] += 1
        time.sleep(5)
    last = {board: boards.status(board) for board in ids}
    blocks = int(last[ids[0]]["height"]) - int(first[ids[0]]["height"])
    print(f"blocks {blocks} in {minutes} minutes, difficulty {last[ids[0]]['difficulty']}")
    for board in ids:
        won = float(last[board]["balance"]) - float(first[board]["balance"])
        print(f"{board[-4:]} hash {sum(rates[board]) / len(rates[board]):.0f} "
              f"balance +{won:.2f} temp {last[board]['temp']} clock {last[board]['clock']} "
              f"no answer {missed[board]}")


if __name__ == "__main__":
    main()
