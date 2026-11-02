#!/usr/bin/env python3
"""Times one signature check on a board, the number under a node's limit: check_time.py BOARD [ROUNDS]."""
import re
import sys

import boards

ROUNDS = 10


def main():
    boards.needs(1, __doc__)
    board = sys.argv[1]
    rounds = int(sys.argv[2]) if len(sys.argv) > 2 else ROUNDS
    clock = boards.status(board).get("clock")
    times = []
    for _ in range(rounds):
        said = boards.line(board, "check-time") or ""
        found = re.search(r"check-time (\d+)ms good=true", said)
        if found:
            times.append(int(found.group(1)))
    if not times:
        sys.exit("The board did not answer. Its firmware may be older than this word.")
    times.sort()
    print(f"clock {clock} MHz, {len(times)} checks, fastest {times[0]} ms, "
          f"middle {times[len(times) // 2]} ms, slowest {times[-1]} ms")


if __name__ == "__main__":
    main()
