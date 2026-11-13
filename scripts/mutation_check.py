#!/usr/bin/env python3
"""Plant one known mistake at a time in an isolated copy and require a named test to fail.

Shipped code and the acceptance contract are never edited. A mutant that survives
means the named test does not actually guard that rule.
"""
import datetime
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

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
    # Falls back to a fresh start on any file header error instead of refusing.
    ("wal_ignore_file_header",STORAGE,
     "        let start = check_file_header(&header, node_id)?;",
     "        let start = check_file_header(&header, node_id).unwrap_or(PRISTINE);",
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
     "                write_atomically(&directory, \"raft.wal\", &file_header(node_id, PRISTINE), \"init_before_rename_raft.wal\")?;",
     "existing_identity_without_wal_fails_closed","raft-storage"),
    # S2: snapshots and compaction (docs/SNAPSHOT_COMPACTION.md section 11).
    ("s2_compact_beyond_applied","crates/raft-core/src/lib.rs",
     "        if index != self.last_applied || index <= self.state.base().index {",
     "        if index <= self.state.base().index {",
     "compaction_only_covers_applied_entries","raft-core"),
    ("s2_reply_before_install_is_durable","crates/raft-core/src/lib.rs",
     "            effects.insert(\n                0,\n                Effect::PersistSnapshot {",
     "            effects.insert(\n                effects.len(),\n                Effect::PersistSnapshot {",
     "follower_installs_only_after_the_snapshot_is_durable","raft-core"),
    ("s2_install_already_covered_snapshot","crates/raft-core/src/lib.rs",
     "                if last_included_index <= self.commit_index {",
     "                if false && last_included_index <= self.commit_index {",
     "a_snapshot_we_already_cover_is_acknowledged_without_change","raft-core"),
    ("s2_no_append_before_base_rule","crates/raft-core/src/lib.rs",
     "                let (check_index, check_term, skip) = if prev_log_index < base.index {",
     "                let (check_index, check_term, skip) = if false && prev_log_index < base.index {",
     "append_entries_that_start_before_the_base_count_as_matching","raft-core"),
    ("s2_chunk_offset_unchecked","crates/raft-core/src/lib.rs",
     "                if receiving.data.len() as u64 != offset {",
     "                if false && receiving.data.len() as u64 != offset {",
     "out_of_order_foreign_and_corrupt_chunks_are_refused","raft-core"),
    ("s2_snapshot_crc_unchecked_on_receive","crates/raft-core/src/lib.rs",
     "                if have != total_len || crc32c(&received.data) != total_crc {",
     "                if have != total_len {",
     "out_of_order_foreign_and_corrupt_chunks_are_refused","raft-core"),
    ("s2_keep_suffix_on_term_mismatch","crates/raft-protocol/src/lib.rs",
     "    let entries = if state.term_at(base.index) == Some(base.term) {",
     "    let entries = if state.term_at(base.index).is_some() {",
     "adopt_snapshot_keeps_suffix_only_when_the_base_matches","raft-protocol"),
    ("s2_wal_rewritten_before_snapshot",STORAGE,
     "            write_atomically(\n                &self.directory,\n                \"snapshot\",\n                &encode_snapshot(snapshot),\n                \"install_before_snapshot_rename\",\n            )?;\n            failpoint(\"install_after_snapshot_before_wal\");\n            self.rewrite_wal(next)",
     "            self.rewrite_wal(next)?;\n            write_atomically(\n                &self.directory,\n                \"snapshot\",\n                &encode_snapshot(snapshot),\n                \"install_before_snapshot_rename\",\n            )?;\n            failpoint(\"install_after_snapshot_before_wal\");\n            Ok(())",
     "real_process_crash_during_snapshot_install_recovers","raft-storage"),
    ("s2_snapshot_file_crc_unchecked",STORAGE,
     "    if crc32c(data) != word(56) {",
     "    if false && crc32c(data) != word(56) {",
     "snapshot_corruption_fails_closed_unchanged","raft-storage"),
    ("s2_accept_wal_newer_than_snapshot",STORAGE,
     "            Some(found) if base.index > found.base.index => {",
     "            Some(found) if false && base.index > found.base.index => {",
     "snapshot_and_wal_consistency_rules","raft-storage"),
    ("s2_older_sequence_executes_again","crates/kv-state/src/lib.rs",
     "            if request.sequence < session.sequence {",
     "            if false && request.sequence < session.sequence {",
     "older_sequence_after_newer_is_rejected_without_effect","kv-state"),
    # S3: ReadIndex (docs/READ_INDEX.md section 10).
    ("s3_answer_without_confirmation","crates/raft-core/src/lib.rs",
     "            Some(read_index) if read.round <= confirmed && last_applied >= read_index => {",
     "            Some(read_index) if last_applied >= read_index => {",
     "replies_to_requests_sent_before_the_read_do_not_confirm_it","raft-core"),
    ("s3_round_not_advanced","crates/raft-core/src/lib.rs",
     "        self.read_round += 1;\n",
     "",
     "replies_to_requests_sent_before_the_read_do_not_confirm_it","raft-core"),
    ("s3_skip_current_term_commit_wait","crates/raft-core/src/lib.rs",
     "        if self.role != Role::Leader\n            || self.state.term_at(self.commit_index) != Some(self.state.term)\n        {",
     "        if self.role != Role::Leader {",
     "read_waits_for_a_committed_entry_of_its_term","raft-core"),
    ("s3_keep_reads_after_losing_leadership","crates/raft-core/src/lib.rs",
     "        for read in self.pending_reads.drain(..) {",
     "        for read in std::iter::empty::<PendingRead>() {",
     "reads_fail_when_leadership_is_lost","raft-core"),
    ("s3_answer_before_applied","crates/raft-core/src/lib.rs",
     "            Some(read_index) if read.round <= confirmed && last_applied >= read_index => {",
     "            Some(read_index) if read.round <= confirmed => {",
     "read_waits_until_the_state_machine_reaches_read_index","raft-core"),
]

def classify(test,stdout,stderr):
    """detected: the named test itself ran and failed. build_error and
    test_not_run mean the test never ran, so they are not detections either;
    survived means it ran and passed with the mistake in place."""
    if re.search(r"^error(\[E\d+\])?: could not compile",stderr,re.MULTILINE):
        return "build_error"
    # The node's trace writes straight to the process's stdout, which libtest's
    # capture does not intercept, so a trace line can split libtest's own
    # "test NAME ... FAILED" line. Accept any of three signals that name the test:
    # the result line, the panic message, or the closing "failures:" list.
    name=rf"(\S+::)?{re.escape(test)}"
    both=stdout+"\n"+stderr
    signals=[rf"^test {name} \.\.\. FAILED$",rf"thread '{name}' (\(\d+\) )?panicked at",rf"^    {name}$"]
    if any(re.search(signal,both,re.MULTILINE) for signal in signals):
        return "detected"
    # A misspelled or removed test name makes cargo run zero tests and succeed;
    # that says nothing about the mutant, so do not call it a survivor.
    if not re.search(rf"^test {name} \.\.\. ",both,re.MULTILINE):
        return "test_not_run"
    return "survived"

def copy_source(work):
    shutil.copytree(ROOT/"crates",work/"crates")
    shutil.copytree(ROOT/"config",work/"config")
    for f in ("Cargo.toml","Cargo.lock","rust-toolchain.toml"):
        if (ROOT/f).exists(): shutil.copy2(ROOT/f,work/f)

def cargo_test(work,test,package):
    # Each copy builds in its own target directory. A shared one let cargo reuse
    # another case's mutated crate: copies of the same workspace get the same
    # metadata hash, and copied files keep their old modification times
    # (docs/lessons/06-mutants-sharing-a-build-directory/).
    env=dict(os.environ)
    env["CARGO_TARGET_DIR"]=str(work/"target")
    scope=["-p",package] if package else ["--workspace"]
    command=["cargo","test",*scope,test]
    result=subprocess.run(command,cwd=work,env=env,capture_output=True,timeout=600)
    (work/"stdout.txt").write_bytes(result.stdout)
    (work/"stderr.txt").write_bytes(result.stderr)
    shutil.rmtree(work/"target",ignore_errors=True)  # created by this run only
    return command,result

def main():
    only=set(sys.argv[1:])
    unknown=only-{name for name,*_ in CASES}
    if unknown: raise SystemExit(f"unknown mutation cases: {sorted(unknown)}")
    cases=[case for case in CASES if not only or case[0] in only]
    # Fail before spending minutes on earlier cases if any anchor has drifted.
    drifted=[name for name,file,original,*_ in cases if (ROOT/file).read_text().count(original)!=1]
    if drifted: raise RuntimeError(f"Mutation anchors missing or not unique: {drifted}")
    _,fingerprint=snapshot(ROOT)
    stamp=datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    destination=ROOT/"artifacts"/"mutations"/stamp
    destination.mkdir(parents=True)
    # Control: without any mutation, every named test must run and pass;
    # otherwise a later failure would say nothing about the planted mistake.
    controls=[]
    for test,package in sorted({(case[4],case[5] or "") for case in cases}):
        work=destination/"control"/(package or "workspace")/test
        copy_source(work)
        command,result=cargo_test(work,test,package or None)
        passed=result.returncode==0 and re.search(rf"^test (\S+::)?{re.escape(test)} \.\.\. ok$",result.stdout.decode(errors="replace"),re.MULTILINE) is not None
        controls.append({"test":test,"package":package or None,"command":command,"exit_code":result.returncode,"passed":passed})
        print(f"control {test}: {'ok' if passed else 'FAILED'}",flush=True)
    if not all(c["passed"] for c in controls):
        raise SystemExit(f"control runs failed: {[c['test'] for c in controls if not c['passed']]}")
    results=[]
    for name,file,original,mutated,test,package in cases:
        work=destination/name
        copy_source(work)
        path=work/file
        source=path.read_text()
        if source.count(original)!=1: raise RuntimeError(f"Mutation anchor not unique: {name}")
        path.write_text(source.replace(original,mutated,1))
        command,result=cargo_test(work,test,package)
        outcome=classify(test,result.stdout.decode(errors="replace"),result.stderr.decode(errors="replace"))
        detected=result.returncode!=0 and outcome=="detected"
        results.append({"case":name,"test":test,"command":command,"exit_code":result.returncode,"outcome":outcome,"detected":detected,"directory":str(work.relative_to(ROOT))})
        print(f"{name}: {outcome.upper() if not detected else 'detected'} ({test})",flush=True)
    if snapshot(ROOT)[1]!=fingerprint: raise RuntimeError("source changed while mutants were running")
    undetected=[r["case"] for r in results if not r["detected"]]
    report={"command":"python scripts/mutation_check.py","date":stamp,"source_fingerprint":fingerprint,
            "scope":"isolated source copies; original shipped code and acceptance unchanged",
            "case_count":len(results),"detected_count":len(results)-len(undetected),"undetected":undetected,
            "isolation":"each copy builds in its own CARGO_TARGET_DIR (since 2026-11-13)",
            "control_count":len(controls),"controls":controls,
            "cases":results,"all_detected":not undetected}
    report["partial"]=bool(only)
    text=json.dumps(report,indent=2)+"\n"
    (destination/"report.json").write_text(text)
    if not only:
        # Only a full run may stand as the acceptance report.
        latest=ROOT/"artifacts"/"reports"/"mutations.json"
        latest.parent.mkdir(parents=True,exist_ok=True)
        latest.write_text(text)
    print(json.dumps({k:v for k,v in report.items() if k!="cases"},indent=2))
    return 0 if report["all_detected"] else 1

if __name__=="__main__": raise SystemExit(main())
