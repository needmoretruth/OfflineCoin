#!/usr/bin/env python3
"""Writes a line each time a board's height changes, with difficulty and seconds since the last: chain_log.py BOARD LOG."""
import sys
import time

import boards


def main():
    boards.needs(2, __doc__)
    board, path = sys.argv[1], sys.argv[2]
    last, at = None, time.time()
    with open(path, "a", encoding="ascii") as log:
        while True:
            try:
                fields = boards.status(board)
            except OSError:
                time.sleep(5)
                continue
            height = fields.get("height")
            if height and height != last:
                now = time.time()
                log.write(f"{time.strftime('%H:%M:%S')} {height} {fields['difficulty']} {now - at:.0f} "
                          f"{fields['hashrate']} {fields['reorg']}\n")
                log.flush()
                last, at = height, now
            time.sleep(4)


if __name__ == "__main__":
    main()
