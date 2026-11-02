#!/usr/bin/env python3
"""Empties a board by moving it to another coin and back, and times its sync: fresh_sync.py BOARD NEIGHBOUR COIN_HEX."""
import sys
import time

import boards


def main():
    board, neighbour, coin = sys.argv[1:4]
    top = int(boards.status(neighbour)["height"])
    print("neighbour height", top, flush=True)
    print(boards.line(board, f"coin {coin}"), flush=True)
    print(boards.line(board, "coin confirm"), flush=True)
    boards.wait_for(lambda: int(boards.status(board).get("height", top)) < 5, 60)
    print("other coin height", boards.status(board).get("height"), flush=True)
    print(boards.line(board, "coin default"), flush=True)
    print(boards.line(board, "coin confirm"), flush=True)
    time.sleep(8)
    start = time.time()
    last = -1
    while time.time() - start < 1800:
        fields = boards.status(board)
        height = int(fields.get("height", -1))
        if height != last:
            print(f"{time.time() - start:.0f} s height {height} sync {fields.get('sync')}", flush=True)
            last = height
        if height >= top and fields.get("tip") == boards.status(neighbour).get("tip"):
            break
        time.sleep(5)
    print("synced in", round(time.time() - start), "seconds", flush=True)


if __name__ == "__main__":
    main()
