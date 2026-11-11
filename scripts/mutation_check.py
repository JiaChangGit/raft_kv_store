#!/usr/bin/env python3
"""Plant one known mistake at a time in an isolated copy and require a named test to fail.

Shipped code and the acceptance contract are never edited. A mutant that survives
means the named test does not actually guard that rule.
"""
import datetime
import json
import os
from pathlib import Path
import shutil
import subprocess

from verify_gate import snapshot

ROOT=Path(__file__).resolve().parents[1]
STORAGE="crates/raft-storage/src/lib.rs"

# (name, file, original text, mutated text, test that must fail, cargo package or None for the workspace)
CASES=[
    ("quorum_one","crates/raft-protocol/src/lib.rs","pub const QUORUM: usize = 2;","pub const QUORUM: usize = 1;","quorum_is_two_of_three",None),
    ("unsafe_old_term_commit","crates/raft-core/src/lib.rs","if self.state.term_at(n) == Some(self.state.term) && count >= QUORUM {","if count >= QUORUM {","previous_term_entry_requires_current_term_commit",None),
    ("reversed_queue","crates/raft-sim/src/lib.rs","let (key, value) = self.queue.pop_first()?;","let (key, value) = self.queue.pop_last()?;","simulation_seed_is_replayable",None),
    ("saturated_pool_spin","crates/raft-node/src/main.rs","                            Instant::now() + PEER_TIMEOUT,\n                        );","                            Instant::now(),\n                        );","saturated_outbound_pool_does_not_starve_status",None),
    # WAL format 2. The first three are the format 1 defect and its close relatives.
    ("wal_skip_record_header_checksum",STORAGE,
     "    if crc32c(&bytes[..20]) != stored {",
     "    if false && crc32c(&bytes[..20]) != stored {",
     "every_single_bit_flip_fails_closed_unchanged","raft-storage"),
    ("wal_truncate_on_bad_header",STORAGE,
     "            let payload_len = check_header(&header, next_sequence)?;",
     "            let payload_len = match check_header(&header, next_sequence) {\n"
     "                Ok(found) => found,\n"
     "                Err(_) => {\n"
     "                    self.remove_torn_tail(offset, length, next_sequence)?;\n"
     "                    break;\n"
     "                }\n"
     "            };",
     "reproduced_length_corruption_fails_closed_unchanged","raft-storage"),
    ("wal_compare_eof_before_header_checksum",STORAGE,
     "            let payload_len = check_header(&header, next_sequence)?;",
     "            let declared = u32::from_le_bytes(header[8..12].try_into().unwrap()) as u64;\n"
     "            if declared > 0\n"
     "                && declared <= MAX_RECORD_PAYLOAD as u64\n"
     "                && remaining < HEADER_LEN as u64 + declared + 4\n"
     "            {\n"
     "                self.remove_torn_tail(offset, length, next_sequence)?;\n"
     "                break;\n"
     "            }\n"
     "            let payload_len = check_header(&header, next_sequence)?;",
     "reproduced_length_corruption_fails_closed_unchanged","raft-storage"),
    ("wal_skip_record_checksum",STORAGE,
     "            if crc32c(&checksum_input) != u32::from_le_bytes(checksum) {",
     "            if false && crc32c(&checksum_input) != u32::from_le_bytes(checksum) {",
     "every_single_bit_flip_fails_closed_unchanged","raft-storage"),
    ("wal_accept_any_partial_header",STORAGE,
     "                check_partial_header(&header[..available], next_sequence)?;\n",
     "",
     "zero_or_garbage_tail_rejected","raft-storage"),
    ("wal_truncate_before_copy_is_durable",STORAGE,
     "        preserve_torn_tail(&self.directory, sequence, &tail)?;\n        self.file.set_len(offset)?;",
     "        self.file.set_len(offset)?;\n        preserve_torn_tail(&self.directory, sequence, &tail)?;",
     "torn_tail_copy_failure_blocks_truncation","raft-storage"),
    ("wal_unbounded_torn_copies",STORAGE,
     "    while older.len() >= MAX_TORN_COPIES {",
     "    while false && older.len() >= MAX_TORN_COPIES {",
     "torn_tail_copies_are_idempotent_and_bounded","raft-storage"),
    ("wal_ignore_file_header",STORAGE,
     "        check_file_header(&header, node_id)?;\n",
     "",
     "file_header_corruption_and_identity_mismatch_rejected","raft-storage"),
    ("wal_adopt_any_wal_without_identity",STORAGE,
     "                if !untouched {",
     "                if false && !untouched {",
     "interrupted_initialization_is_completed_or_rejected","raft-storage"),
    ("wal_recreate_missing_wal",STORAGE,
     "                return Err(io::Error::new(\n"
     "                    io::ErrorKind::NotFound,\n"
     "                    \"identity exists but raft.wal is missing; refusing to assume an empty node\",\n"
     "                ));",
     "                write_atomically(&directory, \"raft.wal\", &file_header(node_id, PRISTINE))?;",
     "existing_identity_without_wal_fails_closed","raft-storage"),
]

def main():
    _,fingerprint=snapshot(ROOT)
    stamp=datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    destination=ROOT/"artifacts"/"mutations"/stamp
    destination.mkdir(parents=True)
    results=[]
    for name,file,original,mutated,test,package in CASES:
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
        scope=["-p",package] if package else ["--workspace"]
        command=["cargo","test",*scope,test,"--","--nocapture"]
        result=subprocess.run(command,cwd=work,env=env,capture_output=True,timeout=300)
        (work/"stdout.txt").write_bytes(result.stdout)
        (work/"stderr.txt").write_bytes(result.stderr)
        observed=result.stdout.decode(errors="replace")
        # A build error is not a detection: the test itself must run and fail.
        detected=result.returncode!=0 and "FAILED" in observed and f"{test} ... FAILED" in observed
        results.append({"case":name,"test":test,"command":command,"exit_code":result.returncode,"detected":detected,"directory":str(work.relative_to(ROOT))})
        print(f"{name}: {'detected' if detected else 'SURVIVED'} by {test}",flush=True)
    if snapshot(ROOT)[1]!=fingerprint: raise RuntimeError("source changed while mutants were running")
    undetected=[r["case"] for r in results if not r["detected"]]
    report={"command":"python scripts/mutation_check.py","date":stamp,"source_fingerprint":fingerprint,
            "scope":"isolated source copies; original shipped code and acceptance unchanged",
            "case_count":len(results),"detected_count":len(results)-len(undetected),"undetected":undetected,
            "cases":results,"all_detected":not undetected}
    text=json.dumps(report,indent=2)+"\n"
    (destination/"report.json").write_text(text)
    latest=ROOT/"artifacts"/"reports"/"mutations.json"
    latest.parent.mkdir(parents=True,exist_ok=True)
    latest.write_text(text)
    print(json.dumps({k:v for k,v in report.items() if k!="cases"},indent=2))
    return 0 if report["all_detected"] else 1

if __name__=="__main__": raise SystemExit(main())
