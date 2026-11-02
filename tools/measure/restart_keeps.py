#!/usr/bin/env python3
"""Checks that cap, brightness and full speed outlive a restart: restart_keeps.py BOARD."""
import sys
import time

import boards


def show(board, note):
    fields = boards.status(board)
    print(f"{note}: limit {fields.get('limit')} chosen {fields.get('chosen')} bright {fields.get('bright')} "
          f"clock {fields.get('clock')} hash {fields.get('hashrate')} boots {fields.get('reset')}", flush=True)


def restart(board):
    power = boards.status(board)["power"]
    boards.line(board, f"power {power}")
    time.sleep(16)


def main():
    board = sys.argv[1]
    show(board, "start")
    for lines, note in ((["limit 700"], "limit 700"), (["bright 30"], "bright 30"),
                        (["full on"], "full on"), (["full off"], "full off"),
                        (["limit default", "bright 64"], "defaults")):
        for line in lines:
            print(line, "->", boards.line(board, line), flush=True)
        time.sleep(16)
        show(board, note)
        restart(board)
        show(board, note + " after restart")


if __name__ == "__main__":
    main()
