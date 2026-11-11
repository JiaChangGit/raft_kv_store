# P03 — WAL, Durable Barriers & Crash Recovery

**Execute:** `bash scripts/run_phase.sh 03`

**Prerequisite:** Previous phase P02 passed and has fresh gate receipt.

## Objective

Add `raft-storage` append-only versioned WAL and connect real storage durability boundaries with raft core effect execution. No successful vote/append RPC result can depend on non-durable state.

## Allowed deliverables

- `crates/raft-storage` + tests; integration with runtime-agnostic effect executor in `raft-sim`.
- `docs/STORAGE_FORMAT_V1.md` with exact byte offsets/version/checksum/crash response matrix.

## Ordered execution steps

1. Implement format v1: magic/version/record_type/length/seq/payload/CRC32C, bounded size, deterministic encoding with explicit hard state and truncate+append transaction. Sentinel index is virtual, not WAL entry.
2. Implement `sync_all` on each successful transaction before satisfying dependent Raft effects. Initial identity metadata validates node/cluster/format. Correctly report write/sync failures without sending success ACK.
3. Recovery scans records, verifies CRC, sequence and length; applies each atomic transaction to *persistent Term/Vote/Log only*. Truncate solely provable incomplete tail; fail closed on ambiguous/in-middle corruption.
4. On Restart set Follower, volatile indexes 0, KV/session empty. No Apply until consensus establishes committed prefix; leader's current-term NoOp ensures knowledge of old prefix before servicing clients.
5. Create failpoints before write, after write before sync, after sync before ACK, after quorum before apply, after apply before client reply. Fault simulation must allow an unacknowledged complete WAL transaction to survive a crash.
6. Use fsync-success hook to mark local durable match; do not update local durability based on mere in-memory append.

## Mandatory real Rust tests

- `vote_after_restart_not_duplicated`: after durable vote and restart a different candidate in same term is rejected.
- `append_ack_after_durable_sync`: ACK effect is impossible until verified WAL fsync completion, including forced sync failure.
- `uncommitted_suffix_not_applied_after_recovery`: restart has persistent log but zero committed KV Apply until learned safely.
- `truncated_wal_tail_safe`: partial tail recovery doesn't invent a record; non-tail corruption fails closed.

## Boundary / acceptance

Do not treat un-fsynced means definitely lost. No direct KV replay from log length. WAL round-trip and crash tests pass on Linux filesystem.

## Obligations shared by every phase

1. Read `AGENTS.md`, `docs/02_ARCHITECTURE_RFC.md`, `docs/03_PROTOCOL_STORAGE.md`, `docs/04_INVARIANTS_TEST_PLAN.md`, and the appropriate earlier code/tests before editing.
2. Do **only** this phase's scope. Do not add a Raft library or introduce multi-node deployment beyond the 3 fixed nodes.
3. Keep code `cargo fmt` clean, `cargo clippy -D warnings` clean, all earlier tests passing; preserve documented types and interfaces.
4. Add **real** negative tests; named test requirements are specified below and in `gates.json`. Do not create empty/stub tests and do not weaken assertions.
5. If a design assumption is impossible, write `docs/BLOCKERS.md` explaining the precise failing invariant and stop instead of silently altering safety contracts.
6. End with a summary of modified files, invariants addressed, commands attempted and genuine outcomes. Do not claim a gate passed unless it actually did.
7. Do not edit `AGENTS.md`, `gates.json`, the provided gate checker or runner to make a failure pass.

## Gate commands (run from workspace root)

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python3 scripts/verify_gate.py 03
```

## Required evidence

- Every Rust test ID and exact pytest node ID listed in `gates.json` for this stage exists and passes.
- Previous stages remain green. All new transitions and persistence boundaries have structured trace or assertions.
- Codex final output reports actual commands and results, not forecasts.
