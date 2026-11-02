#!/usr/bin/env python3
"""One payment and one block each way between two nodes that reach each other only through a
Wi-Fi router, as ordinary clients of it:
router_links.py boards FIRST SECOND
router_links.py desktop REMOTE DIR BOARD

FIRST, SECOND and BOARD are boards on a cable, named as in boards.py, already joined to the router
with every other way between the two ends shut, which the script shows with `near` before and
after. REMOTE is the ssh name of a computer whose node reads typed lines from DIR/in and writes its
log to DIR/node.log, as in desktop_links.py, and mines. Times are seconds. A payment to a board
counts once it waits in the board's pool; a payment to the computer counts once the computer holds
it in a block, since its node prints only what blocks hold. A block counts from the moment the
miner holds it to the moment the other end does; the miner runs alone with a raised cap for it."""
import re
import sys
import time

import boards
from desktop_links import Desktop

PAYMENT_WAIT = 300
BLOCK_WAIT = 1_200
AMOUNT = "2"
QUICK_CAP = "20000"


def board_has(board, tx):
    answer = boards.line(board, f"tx {tx}") or ""
    return re.search(r"\b(pool|height=\d+)", answer) is not None


def pay_from_board(board, address):
    answer = boards.line(board, f"pay {address} {AMOUNT}") or ""
    found = re.search(r"[0-9a-f]{64}", answer)
    if not found:
        sys.exit(f"{board} did not pay: {answer}")
    return found.group(0)


def pay_from_desktop(desktop, address):
    found = desktop.ask(f"pay {address} {AMOUNT}", r"^([0-9a-f]{64})$")
    if not found:
        sys.exit("The computer did not pay.")
    return found.group(1)


def desktop_has_in_block(desktop, tx):
    return desktop.ask(f"explore {tx}", r"^in block\s+\d+", 6) is not None


def board_block(miner, other_has, others_mining_off):
    """The miner finds a block alone; returns seconds from its block to the other end's."""
    for stop in others_mining_off:
        stop()
    boards.line(miner, f"limit {QUICK_CAP}")
    boards.line(miner, "mine on")
    start = boards.status(miner)
    try:
        moved = boards.wait_for(lambda: boards.status(miner).get("tip") != start["tip"], BLOCK_WAIT, 0.5)
        if moved is None:
            return None
        mined = boards.status(miner)
        return boards.wait_for(lambda: other_has(mined), 120, 0.5)
    finally:
        boards.line(miner, "limit default")


def near(*names):
    for name in names:
        print(f"{name}:", boards.line(name, "near"))


def two_boards(first, second):
    near(first, second)
    results = {}
    for payer, payee in ((first, second), (second, first)):
        address = boards.status(payee)["address"]
        tx = pay_from_board(payer, address)
        results[f"payment {payer} to {payee}"] = boards.wait_for(lambda: board_has(payee, tx), PAYMENT_WAIT, 0.5)
    for miner, other in ((first, second), (second, first)):
        def other_has(mined, other=other):
            return boards.status(other).get("tip") == mined["tip"]
        arrived = board_block(miner, other_has, [lambda other=other: boards.line(other, "mine off")])
        results[f"block {miner} to {other}"] = arrived
        boards.line(other, "mine on")
    near(first, second)
    return results


def board_and_desktop(remote, folder, board):
    desktop = Desktop(remote, folder)
    found = desktop.ask("keys", r"^\* \d+\s+(olc[0-9a-f]+)")
    if not found:
        sys.exit("The computer's node did not answer `keys`.")
    near(board)
    results = {}
    tx = pay_from_board(board, found.group(1))
    start = time.time()
    got = boards.wait_for(lambda: desktop_has_in_block(desktop, tx), BLOCK_WAIT, 1)
    results["payment board to computer, in a block"] = got
    tx = pay_from_desktop(desktop, boards.status(board)["address"])
    results["payment computer to board"] = boards.wait_for(lambda: board_has(board, tx), PAYMENT_WAIT, 0.5)

    # The computer mines alone: the board stops, and its new tip can only have come from there.
    boards.line(board, "mine off")
    height = desktop.height() or 0
    moved = boards.wait_for(lambda: (desktop.height() or 0) > height, BLOCK_WAIT, 0.5)
    if moved is not None:
        target = desktop.height()
        results["block computer to board"] = boards.wait_for(
            lambda: int(boards.status(board)["height"]) >= target, 120, 0.5)
    desktop.type("mine off")

    def desktop_has(mined):
        return (desktop.height() or 0) >= int(mined["height"])
    arrived = board_block(board, desktop_has, [])
    results["block board to computer"] = arrived
    desktop.type("mine on")
    print("waited", round(time.time() - start), "s in all")
    near(board)
    return results


def main():
    boards.needs(3, __doc__)
    if sys.argv[1] == "boards":
        results = two_boards(sys.argv[2], sys.argv[3])
    elif sys.argv[1] == "desktop" and len(sys.argv) > 4:
        results = board_and_desktop(sys.argv[2], sys.argv[3], sys.argv[4])
    else:
        sys.exit(__doc__)
    for name, seconds in results.items():
        print(f"{name}: {seconds if seconds is not None else 'did not happen'}")
    if any(seconds is None for seconds in results.values()):
        sys.exit(1)


if __name__ == "__main__":
    main()
