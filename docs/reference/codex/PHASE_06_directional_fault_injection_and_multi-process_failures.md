# P06 — Directional Fault Injection & Multi-Process Failures

**Execute:** `bash scripts/run_phase.sh 06`

**Prerequisite:** Previous phase P05 passed and has fresh gate receipt.

## Objective

Implement a Python 3 frame-aware fault proxy/harness and run real 3-process fault scenarios without bypassing the proxy. Keep deterministic sim cases as regression tests.

## Allowed deliverables

- `tools/fault_proxy.py`, `tools/cluster.py`, `tests/integration/test_faults.py` and any necessary deterministic fixtures.
- `docs/FAULT_SCHEDULE_FORMAT.md` with seeded fault schedule and replay commands.

## Ordered execution steps

1. Proxy understands length-prefixed complete RPC frames and maintains directional A->B / B->A rules. Provide allow, drop, delay, duplicate, reorder full frames, block existing connections; do not reorder raw bytes within a TCP stream.
2. Ensure that all test peer addresses route through proxy, including reconnect paths; include a topology/route validation test.
3. Implement Python SIGKILL/Restart, leader discovery polling, temporary persistent node directories, per-node JSONL traces and fault schedule.
4. Run directed partitions: isolate leader A from B/C while B<->C remain connected; old leader must not complete new PUT/GET; majority elects and commits; heal eventually repairs divergent uncommitted suffix.
5. Test follower lag/catchup, conflicting suffix, delayed/duplicated AE response, two candidates, restart with previously unknown outcome.
6. Use bounded liveness windows under stability and distinguish liveness timeout from violation of Safety; capture all logs on failure.

## Mandatory real Python tests

- `tests/integration/test_faults.py::test_partitioned_leader_cannot_commit_or_read`
- `tests/integration/test_faults.py::test_heal_recovers_and_converges`
- `tests/integration/test_faults.py::test_fault_proxy_does_not_bypass_route`

## Boundary / acceptance

Each fault case runs **actual** three `raft-node` Linux subprocesses. Controlled network partitions must affect both new and existing connections. No successful stale leader GET/PUT.

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
python3 -m pytest -q tests/integration/test_faults.py::test_partitioned_leader_cannot_commit_or_read
python3 -m pytest -q tests/integration/test_faults.py::test_heal_recovers_and_converges
python3 -m pytest -q tests/integration/test_faults.py::test_fault_proxy_does_not_bypass_route
python3 scripts/verify_gate.py 06
```

## Required evidence

- Every Rust test ID and exact pytest node ID listed in `gates.json` for this stage exists and passes.
- Previous stages remain green. All new transitions and persistence boundaries have structured trace or assertions.
- Codex final output reports actual commands and results, not forecasts.
