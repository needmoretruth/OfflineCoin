#!/usr/bin/env python3
"""Splits a board off, logs both sides with difficulty, rejoins: split_rejoin.py KEPT LEFT MINUTES [LOG]."""
import sys
import time

import boards


def row(kept, left):
    one, two = boards.status(kept), boards.status(left)
    return one, two


def main():
    boards.needs(3, __doc__)
    kept, left, minutes = sys.argv[1], sys.argv[2], float(sys.argv[3])
    log = open(sys.argv[4] if len(sys.argv) > 4 else "split.log", "a", encoding="ascii")
    one, two = row(kept, left)
    base = int(one["height"])
    print("start height", base, "difficulty", one["difficulty"], "reorg", two["reorg"], flush=True)
    boards.line(left, "offline on")
    start = time.time()
    while time.time() - start < minutes * 60:
        one, two = row(kept, left)
        log.write(f"{time.time() - start:.0f} kept {one['height']} {one['difficulty']} "
                  f"left {two['height']} {two['difficulty']}\n")
        log.flush()
        time.sleep(10)
    print(f"apart: kept +{int(one['height']) - base} left +{int(two['height']) - base} "
          f"difficulty kept {one['difficulty']} left {two['difficulty']}", flush=True)
    boards.line(left, "offline off")
    start = time.time()
    took = boards.wait_for(lambda: boards.status(left).get("tip") == boards.status(kept).get("tip"), 240, 1)
    two = boards.status(left)
    print("same tip after", took, "seconds, height", two["height"], "reorg", two["reorg"], flush=True)


if __name__ == "__main__":
    main()
