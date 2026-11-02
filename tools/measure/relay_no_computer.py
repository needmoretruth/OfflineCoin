#!/usr/bin/env python3
"""A payment and a block along A-B-C with no node on the computer: relay_no_computer.py A B C AMOUNT."""
import re
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
    cable = boards.cable_on(boards.port_of(a))
    cable.say("near")
    print(cable.hear(1.5).strip())
    cable.say(f"pay {to} {amount}")
    print(cable.hear(2).strip())
    start = time.time()
    while time.time() - start < 420:
        heard = cable.hear(2)
        found = re.search(r"block (\d+)", heard)
        if found:
            print("A has block", found.group(1), "after", round(time.time() - start), "seconds")
            break
    cable.say("status")
    print(re.findall(r"(?:height|balance|pool|peers)=\S+", cable.hear(2)))


if __name__ == "__main__":
    main()
