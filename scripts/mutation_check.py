#!/usr/bin/env python3
"""Demonstrate negative tests in isolated copies; never edits shipped code/gates."""
import datetime
import json
import os
from pathlib import Path
import shutil
import subprocess

ROOT=Path(__file__).resolve().parents[1]

def main():
    stamp=datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    destination=ROOT/"artifacts"/"mutations"/stamp
    destination.mkdir(parents=True)
    cases=[("quorum_one","crates/raft-protocol/src/lib.rs","pub const QUORUM: usize = 2;","pub const QUORUM: usize = 1;","quorum_is_two_of_three"),
           ("unsafe_old_term_commit","crates/raft-core/src/lib.rs","if self.state.term_at(n) == Some(self.state.term) && count >= QUORUM {","if count >= QUORUM {","previous_term_entry_requires_current_term_commit"),
           ("reversed_queue","crates/raft-sim/src/lib.rs","let (key, value) = self.queue.pop_first()?;","let (key, value) = self.queue.pop_last()?;","simulation_seed_is_replayable"),
           ("saturated_pool_spin","crates/raft-node/src/main.rs","                            Instant::now() + PEER_TIMEOUT,\n                        );","                            Instant::now(),\n                        );","saturated_outbound_pool_does_not_starve_status")]
    results=[]
    for name,file,original,mutated,test in cases:
        work=destination/name
        shutil.copytree(ROOT/"crates",work/"crates")
        shutil.copytree(ROOT/"config",work/"config")
        for f in ("Cargo.toml","Cargo.lock","rust-toolchain.toml"):
            if (ROOT/f).exists(): shutil.copy2(ROOT/f,work/f)
        path=work/file
        source=path.read_text()
        if source.count(original)!=1: raise RuntimeError(f"Mutation anchor not unique: {name}")
        path.write_text(source.replace(original,mutated,1))
        env=dict(os.environ)
        env["CARGO_TARGET_DIR"]=str(destination/"build")
        command=["cargo","test","--workspace",test,"--","--nocapture"]
        result=subprocess.run(command,cwd=work,env=env,capture_output=True,timeout=180)
        (work/"stdout.txt").write_bytes(result.stdout)
        (work/"stderr.txt").write_bytes(result.stderr)
        observed=result.stdout.decode(errors="replace")
        detected=result.returncode!=0 and "FAILED" in observed and test in observed
        results.append({"case":name,"test":test,"command":command,"exit_code":result.returncode,"detected":detected,"directory":str(work.relative_to(ROOT))})
    report={"command":"python scripts/mutation_check.py","date":stamp,"scope":"isolated source copies; original shipped code and acceptance unchanged","cases":results,"all_detected":all(r["detected"] for r in results)}
    (destination/"report.json").write_text(json.dumps(report,indent=2)+"\n")
    print(json.dumps(report,indent=2))
    return 0 if report["all_detected"] else 1

if __name__=="__main__": raise SystemExit(main())
