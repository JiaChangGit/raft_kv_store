#!/usr/bin/env python3
"""Quick check before a full acceptance run: execute only the given phases' own
Rust tests and pytest nodes with the same parsing rules as verify_gate.py.

It writes no receipt and proves nothing on its own; it exists so that a test the
checker cannot read (for example a #[should_panic] result line) is found in
minutes instead of at the end of a 40-minute cumulative run.

It also applies the phases' report rules to the reports on disk now. Until the
S5 review a rule the checker could never satisfy (F7: "runs" is a list) only
showed up in the last step of a full run. A report from another source is only
noted: reports are regenerated just before acceptance. A rule that a report on
disk breaks is a problem even when the report is stale (for example a new
mutation case it cannot contain yet): the checker would refuse it as it is, so
regenerate the report and run the preflight again.
"""
import json
import subprocess
import sys

from verify_gate import CONTRACT, ROOT, discover_rust, require_rust_execution, snapshot, validate_report


def run(argv):
    done = subprocess.run(argv, cwd=ROOT, capture_output=True, text=True)
    return done.returncode, done.stdout + "\n" + done.stderr


def check_reports(contract: dict, phase: str, fingerprint: str) -> tuple[list[str], list[str]]:
    """(problems, notes) from the report rules the checker applies at `phase`,
    which are those of every phase up to it, applied to the reports on disk."""
    problems, notes = [], []
    for owner, schemas in sorted(contract.get("reports", {}).items()):
        if owner > phase:
            continue
        for schema in schemas if isinstance(schemas, list) else [schemas]:
            path = ROOT / schema["path"]
            if not path.exists():
                notes.append(f"P{owner} {schema['path']}: missing, not checked")
                continue
            try:
                validate_report(path, {**schema, "source_fingerprint_must_match": False})
            except ValueError as error:
                problems.append(f"P{owner} {schema['path']}: {error}")
                continue
            if schema.get("source_fingerprint_must_match") and json.loads(path.read_text()).get("source_fingerprint") != fingerprint:
                notes.append(f"P{owner} {schema['path']}: from another source; regenerate it before acceptance")
    return problems, notes


def main() -> int:
    contract = json.loads(CONTRACT.read_text())
    phases = sys.argv[1:] or [max(contract["phases"])]
    problems = []
    _, fingerprint = snapshot(ROOT)
    for phase in phases:
        found, notes = check_reports(contract, phase, fingerprint)
        problems += found
        for line in found + notes:
            print(("PROBLEM " if line in found else "NOTE    ") + line)
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
