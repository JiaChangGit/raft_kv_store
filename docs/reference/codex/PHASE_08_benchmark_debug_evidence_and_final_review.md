# P08 — Benchmark, Debug Evidence & Final Review

**Execute:** `bash scripts/run_phase.sh 08`

**Prerequisite:** Previous phase P07 passed and has fresh gate receipt.

## Objective

Produce measured baseline/fault performance and complete reproducible engineering evidence. Do not optimize by weakening durability or GET safety.

## Allowed deliverables

- `tools/benchmark.py`, `tests/integration/test_benchmark_smoke.py`, `docs/IMPLEMENTATION_REPORT.md`, `artifacts/reports/benchmark.json`.
- Final revisions to `docs/VERIFICATION_FINDINGS.md`, `docs/FAULT_SCHEDULE_FORMAT.md` as evidence warrants.

## Ordered execution steps

1. Benchmark fixed workloads: sequential single client, 8 concurrent clients, one follower isolated/slow, leader crash and recovery. Record hardware/WSL/CPU/RAM/OS/compiler, payload size, sample count, WAL sync policy and warmup.
2. Measure PUT/GET p50/p95/p99 client latency, operations/sec, WAL fsync p50/p95/p99, failover time, follower catchup time, RSS/CPU/WAL growth. Never invent observations.
3. Implement `test_benchmark_smoke` which verifies report schema and ability to run a tiny real process benchmark without a permanent service.
4. Write `docs/IMPLEMENTATION_REPORT.md`: module map, Figure 2/3/§5.4.2 mapping, commit proof sketch, storage barrier and crash matrix, failure experiments, known limits, command reproduction, changed ADRs.
5. Verify all gates still pass; include negative test demonstration that deliberately violates current-term commit rule in isolated test scope and is detected (not in shipped code).
6. Record final `benchmark.json` and `verification.json`, each with actual command/date/env, and note missing measurements as **NO-GO** rather than filling placeholders.

## Mandatory real Python test

- `tests/integration/test_benchmark_smoke.py::test_benchmark_smoke`

## Boundary / acceptance

Every earlier Gate remains passing, no claim of formal proof or cloud-scale performance. Publish a reproducible final review and known limitations. If benchmark cannot run because of environment, gate must fail honestly until environment permits it.

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
python3 -m pytest -q tests/integration/test_benchmark_smoke.py::test_benchmark_smoke
python3 scripts/verify_gate.py 08
```

## Required evidence

- Every Rust test ID and exact pytest node ID listed in `gates.json` for this stage exists and passes.
- Previous stages remain green. All new transitions and persistence boundaries have structured trace or assertions.
- Codex final output reports actual commands and results, not forecasts.

## Required report schema

`artifacts/reports/benchmark.json` must have `command`, positive integer `run_count`, nonempty `metrics` object, and nonempty `environment` object. Preserve P07 `verification.json` with valid real run evidence.
