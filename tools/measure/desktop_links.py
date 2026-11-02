#!/usr/bin/env python3
"""One block and one payment each way between a desktop node and one board, over one link:
desktop_links.py REMOTE NEAR FAR [OTHER...] [--amount N] [--dir DIR]

REMOTE is the ssh name of the desktop. Its node must already run with only the link under test,
read typed lines from DIR/in and write its log to DIR/node.log (DIR defaults to olc-node, under
the remote home). NEAR is the board on a cable under test. FAR and every OTHER are boards NEAR
must not hear, so the only way between NEAR and the rest is through the desktop. FAR must be on
a cable too, named by port, name or id as in boards.py; each OTHER is named by its id.

The steps: NEAR stops mining and pays the desktop; a block from the others reaches NEAR through
the desktop; the desktop pays NEAR back; NEAR mines a block and the desktop takes it."""
import re
import shlex
import subprocess
import sys
import time

import boards

BLOCK_WAIT = 900
PAYMENT_WAIT = 120


def option(name, default):
    if name in sys.argv:
        at = sys.argv.index(name)
        value = sys.argv[at + 1]
        del sys.argv[at:at + 2]
        return value
    return default


class Desktop:
    """The node on the desktop, through ssh."""

    def __init__(self, remote, folder):
        self.remote, self.folder = remote, folder

    def _run(self, command):
        done = subprocess.run(["ssh", "-o", "BatchMode=yes", self.remote, command],
                              capture_output=True, text=True, timeout=60)
        return done.stdout

    def type(self, line):
        self._run(f"echo {shlex.quote(line)} >> {self.folder}/in")

    def log_after(self, mark):
        return self._run(f"tail -c +{mark + 1} {self.folder}/node.log")

    def log_size(self):
        return int(self._run(f"stat -c %s {self.folder}/node.log").strip() or 0)

    def ask(self, line, pattern, seconds=20):
        """Types a line and returns the first match of `pattern` the node prints after it."""
        mark = self.log_size()
        self.type(line)
        found = None

        def answered():
            nonlocal found
            found = re.search(pattern, self.log_after(mark), re.M)
            return found is not None
        boards.wait_for(answered, seconds, 1)
        return found

    def height(self):
        found = re.findall(r"height (\d+) peers", self.log_after(max(0, self.log_size() - 4000)))
        return int(found[-1]) if found else None


def coins(text):
    return float(text.replace(",", ""))


def since(start):
    return round(time.time() - start)


def through(gate, board, text):
    """Runs a line on a board with no cable, through the board on a cable named `gate`."""
    command = [boards.OLC, "board", board, *text.split(), "--serial", boards.port_of(gate)]
    subprocess.run(command, capture_output=True, text=True, timeout=120, check=False)


def keep_apart(near, far, others, line_for):
    """Each board acts as if it cannot hear the other, both ways and on every link, so that
    nothing reaches NEAR or leaves it except through the desktop."""
    near_id = boards.status(near)["id"]
    for board in [boards.status(far)["id"], *others]:
        boards.line(near, line_for(board))
    boards.line(far, line_for(near_id))
    for other in others:
        # A board with no cable is named by its id, as `olc boards` lists it. FAR carries the
        # line, since NEAR is about to go unheard.
        through(far, other, line_for(near_id))


def main():
    amount = option("--amount", "3")
    folder = option("--dir", "olc-node")
    boards.needs(3, __doc__)
    remote, near, far, *others = sys.argv[1:]
    desktop = Desktop(remote, folder)
    found = desktop.ask("keys", r"^\* \d+\s+(olc[0-9a-f]+)")
    if not found:
        sys.exit("The desktop node did not answer `keys`.")
    desktop_address = found.group(1)
    near_status = boards.status(near)
    keep_apart(near, far, others, lambda board: f"ignore {board}")
    boards.line(near, "mine off")
    results = {}
    try:
        start = time.time()
        paid = boards.line(near, f"pay {desktop_address} {amount}")
        print("NEAR pays the desktop:", paid)
        paid_id = re.search(r"[0-9a-f]{64}", paid or "")
        if not paid_id:
            sys.exit("NEAR did not pay.")
        # The desktop prints its pool only when its height moves, so ask it for the payment by
        # id. It finds a payment once a block holds it, and the boards that mined that block
        # could only have heard it through the desktop.
        got = boards.wait_for(lambda: desktop.ask(f"explore {paid_id.group(0)}",
                                                  r"^Tx \{", 6) is not None, BLOCK_WAIT, 1)
        results["payment board to desktop, in a block, s"] = got
        start_height = int(near_status["height"])

        def block_through_desktop():
            # NEAR mines nothing and hears only the desktop, so a new tip came through it. The
            # payment leaves NEAR's pool once a block holds it, and the desktop can spend it.
            status = boards.status(near)
            return int(status["height"]) > start_height and status["pool"] == "0" \
                and status["tip"] == boards.status(far)["tip"]
        got = boards.wait_for(block_through_desktop, BLOCK_WAIT, 3)
        results["block desktop to board, s"] = got
        print("NEAR's height:", boards.status(near)["height"], "after", since(start), "s")

        address = near_status["address"]
        answer = desktop.ask(f"pay {address} {amount}", r"^([0-9a-f]{64})$|^(\w.*Error.*|.*refused.*)$")
        print("the desktop pays NEAR:", answer.group(0) if answer else None)
        got = boards.wait_for(lambda: int(boards.status(near)["pool"]) > 0, PAYMENT_WAIT, 1)
        results["payment desktop to board, s"] = got

        boards.line(near, "mine on")
        for board in [far]:
            boards.line(board, "mine off")
        mined_from = int(boards.status(near)["height"])
        balance_from = coins(boards.status(near)["balance"])
        start = time.time()

        def desktop_has_near_block():
            # A block NEAR mined pays NEAR its reward on top of the desktop's payment.
            status = boards.status(near)
            height = desktop.height()
            return int(status["height"]) > mined_from \
                and coins(status["balance"]) > balance_from + float(amount) + 0.5 \
                and height is not None and height >= int(status["height"])
        results["block board to desktop, s"] = boards.wait_for(desktop_has_near_block, BLOCK_WAIT, 3)
        # Which link carried each neighbour's traffic, as NEAR saw it.
        print(boards.line(near, "near"))
    finally:
        boards.line(near, "ignore none")
        boards.line(far, "ignore none")
        for other in others:
            through(far, other, "ignore none")
        for board in (near, far):
            boards.line(board, "mine on")
    for name, seconds in results.items():
        print(f"{name}: {seconds if seconds is not None else 'did not happen'}")
    if any(seconds is None for seconds in results.values()):
        sys.exit(1)


if __name__ == "__main__":
    main()
