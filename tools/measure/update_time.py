#!/usr/bin/env python3
"""Starts an update from the app and prints each stage with its second: update_time.py BOARD_NAME [SECONDS]."""
import sys
import time

import boards


def job(name):
    return next((board for board in boards.boards() if board["name"] == name), {})


def main():
    name = sys.argv[1]
    limit = float(sys.argv[2]) if len(sys.argv) > 2 else 300
    board = job(name)
    print(boards.start_update(board["id"]), flush=True)
    start, last = time.time(), ""
    while time.time() - start < limit:
        now = job(name).get("job")
        if now != last:
            print(round(time.time() - start), now, flush=True)
            last = now
        if now is None and time.time() - start > 10:
            break
        time.sleep(1)
    print("build", job(name).get("build"), flush=True)


if __name__ == "__main__":
    main()
