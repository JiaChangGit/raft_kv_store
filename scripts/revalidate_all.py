#!/usr/bin/env python3
"""Revalidate an already reviewed implementation in order; never calls a model."""
import datetime
import json
import os
from pathlib import Path
import subprocess
import sys
from verify_gate import CONTRACT, RECEIPTS

root=Path(__file__).resolve().parents[1]
out=root/"artifacts"/"revalidation"/datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
out.mkdir(parents=True)
env=dict(os.environ)
env["PATH"]=str(Path.home()/".cargo/bin")+os.pathsep+env.get("PATH","")
rows=[]
for n in sorted(json.loads(CONTRACT.read_text())["phases"]):
    command=[sys.executable,"scripts/verify_gate.py",n]
    print(f"P{n}: running cumulative acceptance",flush=True)
    with (out/f"phase-{n}.log").open("wb") as log:
        done=subprocess.run(command,cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT)
    path=root/RECEIPTS/f"phase-{n}.last-attempt.json"
    result=json.loads(path.read_text()) if path.exists() else {}
    rows.append({"phase":n,"exit_code":done.returncode,"status":result.get("status","ERROR"),"source_fingerprint":result.get("source_fingerprint"),"evidence_directory":result.get("evidence_directory"),"log":str((out/f"phase-{n}.log").relative_to(root))})
    (out/"summary.json").write_text(json.dumps(rows,indent=2)+"\n")
    print(f"P{n}: {rows[-1]['status']} exit={done.returncode}",flush=True)
    if done.returncode:
        print((out/f"phase-{n}.log").read_text()[-6000:],flush=True)
        raise SystemExit(done.returncode)
print(f"All final acceptance phases passed; summary={out.relative_to(root)}/summary.json",flush=True)
