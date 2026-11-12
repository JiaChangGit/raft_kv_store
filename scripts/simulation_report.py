#!/usr/bin/env python3
"""Run the full simulation campaign and save it with the source fingerprint it ran on.

Acceptance phase 11 accepts the report only if the fingerprint equals the source
being accepted, so an old campaign can never vouch for new code.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

from verify_gate import ROOT, snapshot

def main() -> int:
    seeds, events = os.environ.get("RAFT_SIM_SEEDS", "1000"), os.environ.get("RAFT_SIM_EVENTS", "10000")
    _, fingerprint = snapshot(ROOT)
    with tempfile.TemporaryDirectory() as work:
        raw = Path(work) / "campaign.json"
        command = ["cargo", "run", "--release", "--locked", "-q", "-p", "raft-sim", "--",
                   "--seeds", seeds, "--events", events, "--output", str(raw)]
        done = subprocess.run(command, cwd=ROOT)
        if not raw.exists():
            print("simulation produced no report", file=sys.stderr)
            return 1
        report = json.loads(raw.read_text())
    if snapshot(ROOT)[1] != fingerprint:
        print("source changed while the simulation ran", file=sys.stderr)
        return 1
    report.update(command=" ".join(command[:-2]), source_fingerprint=fingerprint, exit_code=done.returncode)
    out = ROOT / "artifacts" / "reports" / "simulation.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, indent=2) + "\n")
    summary = {k: report[k] for k in ("seed_count", "event_count", "pass_count", "snapshots_taken", "snapshots_installed")}
    print(json.dumps({**summary, "failures": len(report["failures"]), "output": str(out.relative_to(ROOT))}, indent=2))
    return 0 if done.returncode == 0 and not report["failures"] else 1

if __name__ == "__main__":
    raise SystemExit(main())
