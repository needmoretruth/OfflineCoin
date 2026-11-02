#!/usr/bin/env python3
"""Proves relay on a forced line A-B-C with the peer ignore list: forced_line.py A B C AMOUNT."""
import sys
import time

import boards


def main():
    boards.needs(4, __doc__)
    a, b, c, amount = sys.argv[1:5]
    to = boards.status(c)["address"]
    boards.line(a, f"ignore {boards.status(c)['id']}")
    boards.line(c, f"ignore {boards.status(a)['id']}")
    for board in (a, b):
        boards.line(board, "mine off")
    print("a hears:", boards.line(a, "near"))
    start_height = int(boards.status(c)["height"])
    answer = boards.line(a, f"pay {to} {amount}")
    print("paid:", answer)
    sent = time.time()
    arrived = boards.wait_for(lambda: int(boards.status(c)["pool"]) > 0, 60, 0.5)
    print("seconds until C holds the payment:", arrived)
    mined = boards.wait_for(lambda: int(boards.status(c)["height"]) > start_height, 400, 3)
    print("seconds until C mines:", mined)
    tip = boards.status(c)["tip"]
    print("seconds until A has C's block:", boards.wait_for(
        lambda: boards.status(a)["tip"] == tip, 120, 0.5))
    print("total seconds:", round(time.time() - sent))
    for board in (a, b):
        boards.line(board, "mine on")
    boards.line(a, "ignore none")
    boards.line(c, "ignore none")


if __name__ == "__main__":
    main()
