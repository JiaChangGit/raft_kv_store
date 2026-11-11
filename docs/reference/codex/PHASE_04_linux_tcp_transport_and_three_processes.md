# P04 — Linux TCP Transport & Three Processes

**Execute:** `bash scripts/run_phase.sh 04`

**Prerequisite:** Previous phase P03 passed and has fresh gate receipt.

## Objective

Run the same core in three separate WSL2 Ubuntu processes with TCP-based RPC communication; allow real election, replication, persistent restart, and status inspection. KV client API can remain read-only status/no-op until P05.

## Allowed deliverables

- `crates/raft-transport`, `crates/raft-node`, `config/cluster.toml`.
- `tests/integration/test_cluster_smoke.py` using Python subprocess and stable temporary directories.

## Ordered execution steps

1. TCP framing is `u32_be length + JSON`, max 1MiB. Accept partial reads/writes and multiple frames per connection; reject malformed/too-large frames before unbounded allocation.
2. Single-threaded Raft actor owns role, term, log, indexes; socket receivers/timers enqueue events. Actor executes persistent barrier before sending dependent RPC/ACK. IO threads must not mutate raft core.
3. Use peer rpc_id, local leader epoch, term and request metadata to associate replies; timeout/reconnect doesn't invent ACKs. One AppendEntries outstanding per peer, even when TCP reconnect occurs.
4. Parse immutable config for 3 nodes, unique peer/client endpoints and unique storage dirs. Use monotonic clock for timers. Expose debug-only status endpoint including term, role, last log, commit, apply and WAL durable seq.
5. Python subprocess launches three nodes, observes election with polling (not hard-coded sleep outcomes), kills the leader via SIGKILL, verifies remaining nodes elect a new leader, restarts old node with same data dir.
6. Keep all side effects in actor; log every RPC term/index, durable ACK and state transition. No new CRUD or alternative replication logic.

## Mandatory real Rust tests

- `tcp_framing_handles_partial_io`: fragmented header/payload, concatenated frames, excessive size all covered.
- `rpc_correlation_ignores_stale_reply`: old connection/term/epoch reply cannot move matchIndex.

## Mandatory real Python test

- `tests/integration/test_cluster_smoke.py::test_three_process_election_and_restart`

## Boundary / acceptance

Cluster must consist of **three independent Linux PIDs**; integration test must read the PID list and verify uniqueness, not pretend simulated nodes are processes. The full test must pass in WSL2 with `pytest` installed.

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
python3 -m pytest -q tests/integration/test_cluster_smoke.py::test_three_process_election_and_restart
python3 scripts/verify_gate.py 04
```

## Required evidence

- Every Rust test ID and exact pytest node ID listed in `gates.json` for this stage exists and passes.
- Previous stages remain green. All new transitions and persistence boundaries have structured trace or assertions.
- Codex final output reports actual commands and results, not forecasts.
