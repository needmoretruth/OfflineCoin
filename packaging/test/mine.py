#!/usr/bin/env python3
"""Mines real blocks with the packaged programs, away from the app's own folders.

Usage: mine.py <installed olc-core> <unpacked olc> <folder for results>
The app's own node process mines first, then the terminal program. Each must find a block.
"""
import json
import subprocess
import sys
import tempfile
import threading
from pathlib import Path

WAIT_SECONDS = 180
END_SECONDS = 30


def app_node(core, olc, work):
    """The node process the app starts for itself. It prints its status as JSON once a second."""
    coin = work / "coin.json"
    subprocess.run([olc, "--data", str(work / "empty"), "save-coin", str(coin)], check=True)
    folder = str(work / "app")
    node = subprocess.Popen(
        [core, "node", "--data", folder, "--wallet", folder, "--coin", str(coin),
         "--port", "17311", "--mine", "--cap", "0"],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
    limit = threading.Timer(WAIT_SECONDS, node.kill)
    limit.start()
    found = None
    for line in node.stdout:
        status = json.loads(line)
        if status["blocks_mined"] > 0:
            found = status
            break
    limit.cancel()
    if found is None:
        raise SystemExit("The app's node found no block.")
    node.stdin.write("quit\n")
    node.stdin.flush()
    code = node.wait(END_SECONDS)
    if code != 0:
        raise SystemExit(f"The app's node ended with code {code}.")
    return (f"olc-core node: {found['blocks_mined']} mined, height {found['height']}, "
            f"tip {found['tip']}, {found['hashrate']} H/s, ended with code 0")


def terminal_node(olc, work):
    """The terminal program. It prints a line each time its chain moves."""
    node = subprocess.Popen(
        [olc, "node", "--data", str(work / "terminal"), "--port", "17312", "--mine",
         "--hash-limit", "0"],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
    limit = threading.Timer(WAIT_SECONDS, node.kill)
    limit.start()
    found = None
    for line in node.stdout:
        words = line.split()
        if words[:1] == ["height"] and int(words[1]) > 0:
            found = line.strip()
            break
    limit.cancel()
    node.kill()
    node.wait(END_SECONDS)
    if found is None:
        raise SystemExit("The terminal program found no block.")
    return f"olc node: {found}"


def main():
    core, olc, results = sys.argv[1:4]
    with tempfile.TemporaryDirectory() as folder:
        work = Path(folder)
        lines = [app_node(core, olc, work), terminal_node(olc, work)]
    text = "\n".join(lines) + "\n"
    print(text, end="")
    Path(results, "mined.txt").write_text(text)


if __name__ == "__main__":
    main()
