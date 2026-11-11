# P07 — Independent History Checker & Randomized Verification

**Execute:** `bash scripts/run_phase.sh 07`

**Prerequisite:** Previous phase P06 passed and has fresh gate receipt.

## Objective

Implement an independent Python linearizability oracle and serious adversarial state-machine simulation, including unknown operations and randomized crash/partition schedule replay.

## Allowed deliverables

- `tools/linearizability.py`, `tests/history/test_checker.py`, `tests/integration/test_randomized.py`.
- `artifacts/reports/verification.json` with real run metadata and precise pass counts; `docs/VERIFICATION_FINDINGS.md`.

## Ordered execution steps

1. Write sequential KV reference semantics in Python **independent from Rust Core**; ingest operation records with invoke, response, status and return value.
2. Check real-time precedence for non-overlapping completed operations; enumerate legal linearizations for small concurrent histories. Pending/unknown writes can be absent or be placed if consistent. Document bounded search limit and what was/was not proven.
3. Add known invalid histories; checker MUST reject a stale read and logically conflicting outcomes. Add known valid histories including pending unknown operation and duplicated retries.
4. Add seeded randomized Raft sim traces (target >=1000 seeds, 10000 events/seed as tunable baseline) and invariant check I1..I5. Preserve seed, event trace and first failing invariant.
5. Real 3-process integration: mix concurrent clients, faults, crash/restart, delayed frames; capture history and run checker. Confirm no Safety failures across executed runs; liveness measured separately.
6. Generate `verification.json` with actual executable/version/seed_count/event_count/pass_count/failures and a command to reproduce. No fabricated numbers or empty report.

## Mandatory real Rust test

- `simulation_randomized_invariants`: seed-driven I1..I5 assertions on actual core (not duplicate algorithm).

## Mandatory real Python tests

- `tests/history/test_checker.py::test_linearizability_checker_rejects_stale_read`
- `tests/history/test_checker.py::test_pending_operation_has_unknown_outcome`
- `tests/integration/test_randomized.py::test_seeded_fault_history_is_linearizable`

## Boundary / acceptance

Explicitly list unknown/pending operations, not just success-only histories. If checker fails, save counterexample and **do not** narrow the checker to ignore it.

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
python3 -m pytest -q tests/history/test_checker.py::test_linearizability_checker_rejects_stale_read
python3 -m pytest -q tests/history/test_checker.py::test_pending_operation_has_unknown_outcome
python3 -m pytest -q tests/integration/test_randomized.py::test_seeded_fault_history_is_linearizable
python3 scripts/verify_gate.py 07
```

## Required evidence

- Every Rust test ID and exact pytest node ID listed in `gates.json` for this stage exists and passes.
- Previous stages remain green. All new transitions and persistence boundaries have structured trace or assertions.
- Codex final output reports actual commands and results, not forecasts.

## Required report schema

`artifacts/reports/verification.json` must have `command`, positive integer `seed_count`, `event_count`, `pass_count`, and `failures: []`, populated from a real executed test.
