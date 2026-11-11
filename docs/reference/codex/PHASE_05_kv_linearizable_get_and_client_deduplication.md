# P05 — KV, Linearizable GET & Client Deduplication

**Execute:** `bash scripts/run_phase.sh 05`

**Prerequisite:** Previous phase P04 passed and has fresh gate receipt.

## Objective

Implement actual Put/Get/Delete on deterministic KV state machine, replicated session table, and safe client transport semantics. All new GETs MUST be replicated log entries.

## Allowed deliverables

- `crates/kv-state`, client protocol/routing under `raft-node`, `tools/kv_client.py`, `tests/integration/test_client_retry.py`.
- `docs/CLIENT_CONTRACT.md` containing exact sequence and timeout semantics.

## Ordered execution steps

1. Define deterministic command parser and bounds: key <=256 bytes, value <=4096 bytes; invalid arguments returned before a log entry is accepted.
2. Implement NoOp/Put/Delete/Get `apply(entry)` in order, remembering full result record per `(client_id,seq)` and canonical request content; replay of committed prefix reconstructs KV+Session.
3. Implement Session Table full retention (no GC). Duplicate command same ID+content returns original result; duplicate ID+different content returns REQUEST_ID_REUSE with no modification.
4. Only one logical in-flight request per client; Python client retains same ID/sequence across retry. Multiple duplicated *log* entries are allowed; state machine effect happens once.
5. New GET always appended and must be committed/applied before success; never serve local Follower/old-Leader reads; no ReadIndex and no lease optimization.
6. Timeout=UNKNOWN_OUTCOME (not automatic failure). Status `NOT_LEADER` may include a hint. Crash after commit before client reply must be retryable safely after leader change.
7. Successful client reply only after valid commitment and **local apply**. Do not use stale leader cache to answer successful requests without safe quorum/commit confirmation.

## Mandatory real Rust tests

- `retry_same_request_has_single_effect`: same request id logged twice has one logical effect and same result.
- `replicated_get_linearizable`: GET read point is committed/applied log order; stale leader lacks quorum cannot finish new GET.
- `same_id_different_command_rejected`: ID reuse with different canonical command is deterministic rejection.

## Mandatory real Python test

- `tests/integration/test_client_retry.py::test_retry_after_leader_crash`

## Boundary / acceptance

Client API is usable on real 3-process cluster; no new GET success on isolated previous Leader. No session cache-only correctness assumptions.

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
python3 -m pytest -q tests/integration/test_client_retry.py::test_retry_after_leader_crash
python3 scripts/verify_gate.py 05
```

## Required evidence

- Every Rust test ID and exact pytest node ID listed in `gates.json` for this stage exists and passes.
- Previous stages remain green. All new transitions and persistence boundaries have structured trace or assertions.
- Codex final output reports actual commands and results, not forecasts.
