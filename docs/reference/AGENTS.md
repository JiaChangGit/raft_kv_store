# AGENTS.md — Codex Mandatory Project Rules

You are implementing a **3-process, fixed-membership Rust Raft replicated KV store** in this repository. Treat documents as binding engineering requirements, especially the Raft paper's Figure 2/Figure 3 and §5.4.

## Non-negotiable constraints

1. Implement Raft consensus **from scratch**; never add `openraft`, `raft-rs`, `tikv/raft`, etc.
2. Fixed node IDs 1,2,3; quorum 2; one cluster / one Raft group. No snapshots/compaction, membership changes, sharding, TLS, Web UI, external consensus systems or Kubernetes.
3. One actual `raft-core` is used in simulation and Linux runtime; **do not duplicate consensus logic**.
4. Own consensus state inside one event-loop thread. Transport/Timer/Storage may emit messages/effects but must not directly mutate Raft state.
5. Persist `term`, `vote`, and log mutations before sending RPC responses that depend on them. WAL durable failures fail closed; never silently continue.
6. Current-term commit restriction: majority replication alone DOES NOT directly commit older-term entries. `log[N].term == currentTerm` must hold when leader advances commitIndex to N.
7. Leader's own matchIndex counts **only durable local entries**. Follower ACK counts **only verified durable matching prefix**. Ignore stale RPC replies or replies from old terms/leadership epochs.
8. Restart always starts Follower with volatile `commit_index=0`, `last_applied=0`; only apply provably committed entries, never replay the entire WAL directly into KV.
9. V1 GET is a replicated log command (not local read; not ReadIndex). All successful reads/writes require a safe committed/apply path.
10. Client idempotency: client ID + monotonic sequence, one outstanding logical request per client; durable-by-replay session results; a duplicate must not execute the command twice. Never answer successful requests from a deposed or isolated leader's cache.
11. Avoid `unsafe` Rust unless a narrow, documented and reviewed necessity appears (should not in V1).
12. All transport paths under fault-test go through directional proxies. A TCP stream may fragment bytes; only full frames may be delayed/dropped/duplicated/reordered in the simulated proxy.
13. Tests are required, especially negative safety tests. Do not mark gates passed by creating empty/stub tests or weakening assertions.
14. Maintain stable interfaces, concise comments at subtle invariants, structured logging and test reproducibility with seeds.
15. Only work on the phase specified by the caller. Never skip ahead or make phase scope expansions without a documented blocking dependency.
16. Respect fixed bounds: key <=256 bytes, value <=4096 bytes, frame <=1MiB; reject malformed inputs before allocating unbounded memory.

## Required output from each phase

- Deliver implementation and tests for that phase; format + unit/integration tests where specified.
- Explain invariants touched, code files modified, tests run, actual pass/fail, remaining risks and deviations.
- Do not claim passing results without showing the commands and exit codes.
- Do not edit `gates.json`, `scripts/verify_gate.py`, `scripts/run_phase.sh`, `AGENTS.md` to bypass requirements.
- Use repo-relative paths; no writes outside workspace unless explicitly authorized.
- Do not automatically run later phases; wait for the independent gate check.

## Engineering conventions

- Cargo workspace packages: `raft-core`, `raft-protocol`, `raft-storage`, `kv-state`, `raft-sim`, `raft-transport`, `raft-node` as the phases introduce them.
- Primary Rust tests are `#[test]` or appropriate supported async test and named exactly as in `gates.json`. No invented binary output.
- Python integration uses `python3 -m pytest` and the filenames referenced in `gates.json`.
- No liveness claim for permanently unstable network; safety is assessed separately from liveness.
- Use Linux monotonic timers and separate stdout/stderr for each node.

Read `docs/02_ARCHITECTURE_RFC.md`, `docs/03_PROTOCOL_STORAGE.md`, `docs/04_INVARIANTS_TEST_PLAN.md`, and the current `codex/PHASE_XX_*.md` before editing code.
