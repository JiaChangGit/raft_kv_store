# P00 — Foundation & Deterministic Harness

**Execute:** `bash scripts/run_phase.sh 00`

**Prerequisite:** None (fresh repository).

## Objective

Create a fresh Cargo workspace with packages `raft-protocol`, `raft-core`, `raft-sim`. Freeze IDs, log indexing and event/effect vocabulary; build a virtual-time seeded event queue that can be unit tested **without** an actual Raft election. Do not write production TCP/WAL/clients yet.

## Allowed deliverables

- Root `Cargo.toml` and optional `Cargo.lock`; `crates/raft-protocol`, `crates/raft-core`, `crates/raft-sim`.
- `tests/` test fixtures as appropriate; `docs/API_CONTRACT.md` with exact Rust type signatures and error variants.
- Minimal deterministic event generator/scheduler and `MockDisk` explicitly separate durable/volatile state.

## Ordered execution steps

1. Define immutable cluster configuration for IDs 1,2,3; quorum constant 2, sentinel index=0, entry indexing from 1. Define `Role`, `Term`, `LogIndex`, `LogEntry`, and `Command` including NoOp, Put, Delete, Get as pure protocol data. Reject node IDs outside {1,2,3}.
2. Model RPC messages as strongly typed serde structs, not via arbitrary JSON in core. Declare `Event`, `Effect` and storage barrier identifiers; document how persisted effects order with Send/Reply/Apply.
3. Design `RaftCore` with no socket/fs/Instant calls. Core may be partial/inert at P00; do not fake a leader election.
4. Create simulator queue with monotonic logical `(virtual_time, sequence)` ordering; make pseudo-random choices seed-driven and reproducible. Include `deliver/drop/delay/duplicate` scheduling hooks.
5. Add an independent trace recorder and basic quorum/config assertions. Document assumptions and test invocation.

## Mandatory real Rust tests

- `simulation_seed_is_replayable`: same seed+same scripted events => identical ordered trace (including ties).
- `quorum_is_two_of_three`: exactly 2/3 voters form majority, 1/3 does not.

## Boundary / acceptance

`cargo test --workspace` passes; `cargo test --workspace -- --list` discovers both names. No TCP, no persistence claim, no production Raft logic yet. **Do not** simulate election by writing a second consensus implementation.

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
python3 scripts/verify_gate.py 00
```

## Required evidence

- Every Rust test ID and exact pytest node ID listed in `gates.json` for this stage exists and passes.
- Previous stages remain green. All new transitions and persistence boundaries have structured trace or assertions.
- Codex final output reports actual commands and results, not forecasts.
