#!/usr/bin/env python3
"""Read latest receipts, never substitute an older PASS for the latest failure."""
import json
from pathlib import Path
from verify_gate import CONTRACT, RECEIPTS, snapshot

root=Path(__file__).resolve().parents[1]
_,current=snapshot(root)
for name in sorted(json.loads(CONTRACT.read_text())["phases"]):
    path=root/RECEIPTS/f"phase-{name}.last-attempt.json"
    row=json.loads(path.read_text()) if path.exists() else {}
    freshness="current source" if row.get("source_fingerprint")==current else "source changed / not run"
    print(f"P{name}: {row.get('status','NOT_RUN')} [{freshness}]  {row.get('finished_at','')}  {row.get('evidence_directory','')}")
