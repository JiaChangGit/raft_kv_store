#!/usr/bin/env python3
"""Rebuild the source snapshot in a new target directory and Python venv."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time

from verify_gate import ROOT, snapshot


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir",type=Path,required=True)
    args=parser.parse_args()
    output=args.output_dir.resolve()
    output.relative_to((ROOT/"artifacts").resolve())
    output.mkdir(parents=True,exist_ok=False)
    source=output/"source"
    source.mkdir()
    files,fingerprint=snapshot(ROOT)
    result={"status":"FAIL","started_utc":datetime.now(timezone.utc).isoformat(),
            "source_fingerprint":fingerprint,"commands":[],
            "isolation":{"new_source_copy":True,"new_target":True,"new_venv":True,
                         "shared_host_rustup_and_package_caches":True},
            "limits":["Not a fresh OS or a cold-cache/offline build.",
                      "Does not replay original unavailable gates or copy old receipts into a new project."]}
    environment=dict(os.environ)
    for key in list(environment):
        if key.startswith(("RAFT_","PYTHON")) or key in {"VIRTUAL_ENV","CARGO_TARGET_DIR","CARGO_ENCODED_RUSTFLAGS","RUSTFLAGS"}:
            environment.pop(key,None)
    environment["PATH"]=str(Path.home()/".cargo/bin")+os.pathsep+environment.get("PATH","")
    environment["PYTHONNOUSERSITE"]="1"
    environment["CARGO_TARGET_DIR"]=str(source/"target")
    def save():
        (output/"result.json").write_text(json.dumps(result,indent=2)+"\n")
    def run(argv,label,timeout=600):
        index=len(result["commands"])
        row={"label":label,"argv":[str(x) for x in argv],"cwd":str(source)}
        result["commands"].append(row)
        start=time.monotonic()
        try:
            completed=subprocess.run(row["argv"],cwd=source,env=environment,capture_output=True,timeout=timeout)
            stdout,stderr,code=completed.stdout,completed.stderr,completed.returncode
        except subprocess.TimeoutExpired as error:
            stdout,stderr,code=error.stdout or b"",error.stderr or b"",124
        for name,contents in (("stdout",stdout),("stderr",stderr)):
            path=output/f"{index:02}-{name}.txt"
            path.write_bytes(contents)
            row[name]=str(path.relative_to(output))
        row.update(exit_code=code,elapsed_seconds=time.monotonic()-start,capture_complete=code!=124)
        save()
        print(f"[{label}] exit={code}",flush=True)
        if code: raise RuntimeError(f"{label} failed; inspect {output}")
    try:
        for name,expected in files.items():
            data=(ROOT/name).read_bytes()
            if hashlib.sha256(data).hexdigest()!=expected: raise RuntimeError(f"source changed while copying: {name}")
            destination=source/name
            destination.parent.mkdir(parents=True,exist_ok=True)
            destination.write_bytes(data)
        if snapshot(source)[1]!=fingerprint: raise RuntimeError("copy differs from source snapshot")
        (output/"source-files.json").write_text(json.dumps(files,indent=2)+"\n")
        run([sys.executable,"-m","venv",source/".venv"],"new Python environment")
        python=source/".venv/bin/python"
        run([python,"-m","pip","install","-r","requirements-dev.txt"],"install declared Python dependencies")
        run([python,"-m","pip","freeze"],"Python dependency versions")
        run(["rustc","-Vv"],"Rust version")
        run(["cargo","fmt","--all","--","--check"],"format")
        run(["cargo","clippy","--locked","--workspace","--all-targets","--","-D","warnings"],"clippy")
        run(["cargo","test","--locked","--workspace"],"Rust tests")
        run(["cargo","build","--locked","--workspace","--bins"],"real process executables")
        run([python,"-m","pytest","-q","tests","scripts/test_verify_gate.py","scripts/test_check_lessons.py","scripts/test_mutation_check.py",f"--junitxml={output/'pytest.xml'}"],"Python tests")
        run([python,"-m","tools.learning_demo","--output-dir",source/"artifacts/runs/clean-demo"],"client CLI and crash/restart")
        demo=json.loads((source/"artifacts/runs/clean-demo/result.json").read_text())
        if demo["status"]!="PASS": raise RuntimeError("clean demo did not pass")
        if snapshot(source)[1]!=fingerprint or snapshot(ROOT)[1]!=fingerprint:
            raise RuntimeError("source changed during clean rebuild")
        result.update(status="PASS",source_unchanged=True,demo="source/artifacts/runs/clean-demo/result.json")
    except (OSError,ValueError,RuntimeError) as error:
        result["error"]=str(error)
    finally:
        result["finished_utc"]=datetime.now(timezone.utc).isoformat()
        save()
    return 0 if result["status"]=="PASS" else 1


if __name__=="__main__": raise SystemExit(main())
