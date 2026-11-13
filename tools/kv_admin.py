#!/usr/bin/env python3
"""Cluster membership administration (docs/MEMBERSHIP.md §10).

    tools/kv_admin.py --config cluster.toml status
    tools/kv_admin.py --config cluster.toml add 4
    tools/kv_admin.py --config cluster.toml remove 2

Reads the address book from the cluster's TOML file, finds the leader and asks
it to add or remove one node. Bootstrap exactly one node, once
(raft-node --bootstrap); start every other node with an empty data directory
and add it here. To replace a failed node, add the new one first, then remove
the old one (dissertation §4.4). Stop a node only after it has been removed,
and never re-add an ID with a wiped data directory (MEMBERSHIP.md N2).
"""
import argparse
import json
from pathlib import Path
import sys
import time
import tomllib

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from tools.kv_client import call  # noqa: E402

RETRY = ("NOT_LEADER", "CHANGE_IN_PROGRESS")


def address_book(path):
    config = tomllib.loads(Path(path).read_text())
    return {node["id"]: node["client_addr"] for node in config["nodes"]}


def statuses(book):
    found = {}
    for node, address in sorted(book.items()):
        try:
            found[node] = call(address, {"status": True}, timeout=.5)
        except (OSError, EOFError, ValueError):
            found[node] = None
    return found


def leader(book):
    """The node reporting itself leader with the highest term, if any."""
    leaders = [(s["term"], n) for n, s in statuses(book).items() if s and s["role"] == "Leader"]
    return max(leaders)[1] if leaders else None


def change(book, kind, node, timeout=60.0):
    deadline = time.monotonic() + timeout
    answer = {"status": "NOT_LEADER", "result": None}
    while time.monotonic() < deadline:
        target = leader(book)
        if target is not None:
            try:
                answer = call(book[target], {"version": 1, "membership": {kind: node}}, timeout=17)
            except (OSError, EOFError, ValueError):
                answer = {"status": "TIMEOUT_UNKNOWN", "result": None}
            if answer["status"] == "TIMEOUT_UNKNOWN":
                # Unknown outcome: asking again answers OK at once if it took effect.
                continue
            if answer["status"] not in RETRY:
                return answer
        time.sleep(.2)
    return answer


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--config", required=True, help="the cluster TOML file (address book)")
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("status", help="print the leader's view of the configuration")
    for name in ("add", "remove"):
        sub.add_parser(name).add_argument("node", type=int, choices=range(1, 8))
    args = parser.parse_args()
    book = address_book(args.config)
    if args.command == "status":
        target = leader(book)
        if target is None:
            print(json.dumps({"leader": None}))
            return 1
        s = statuses(book)[target]
        print(json.dumps({"leader": target, "term": s["term"], "voters": s["voters"],
                          "config_index": s["config_index"], "config_committed": s["config_committed"],
                          "catching_up": s["catching_up"]}))
        return 0
    answer = change(book, "Add" if args.command == "add" else "Remove", args.node)
    print(json.dumps(answer))
    if answer["status"] != "OK":
        return 1
    size = len(answer["result"].split(","))
    if size < 3:
        print(f"warning: the configuration has {size} node(s) and cannot tolerate a failure", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
