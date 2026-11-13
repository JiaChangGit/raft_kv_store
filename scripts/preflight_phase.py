#!/usr/bin/env python3
"""Quick check before a full acceptance run: execute only the given phases' own
Rust tests and pytest nodes with the same parsing rules as verify_gate.py.

It writes no receipt and proves nothing on its own; it exists so that a test the
checker cannot read (for example a #[should_panic] result line) is found in
minutes instead of at the end of a 40-minute cumulative run.
"""
import json
import subprocess
import sys

from verify_gate import CONTRACT, ROOT, discover_rust, require_rust_execution


def run(argv):
    done = subprocess.run(argv, cwd=ROOT, capture_output=True, text=True)
    return done.returncode, done.stdout + "\n" + done.stderr


def main() -> int:
    contract = json.loads(CONTRACT.read_text())
    phases = sys.argv[1:] or [max(contract["phases"])]
    problems = []
    for phase in phases:
        body = contract["phases"][phase]
        code, listing = run(["cargo", "test", "--workspace", "--", "--list"])
        if code:
            print(listing[-3000:])
            return 1
        try:
            selected = discover_rust(listing, body["rust_tests"])
        except ValueError as error:
            problems.append(f"P{phase} discovery: {error}")
            continue
        for leaf, full in selected.items():
            code, output = run(["cargo", "test", "--workspace", full, "--", "--exact"])
            try:
                require_rust_execution(output, full)
                print(f"P{phase} ok   {leaf}")
            except ValueError as error:
                problems.append(f"P{phase} {leaf}: {error} (exit {code})")
                print(f"P{phase} FAIL {leaf}")
        for node in body["pytest_nodes"]:
            code, output = run([sys.executable, "-m", "pytest", "-q", node])
            print(f"P{phase} {'ok  ' if code == 0 else 'FAIL'} {node}")
            if code:
                problems.append(f"P{phase} {node}: exit {code}")
    for problem in problems:
        print("PROBLEM", problem)
    print(f"preflight: {len(problems)} problem(s)")
    return 1 if problems else 0


if __name__ == "__main__":
    raise SystemExit(main())
