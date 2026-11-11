# P01 — Election, Term & Vote Safety

**Execute:** `bash scripts/run_phase.sh 01`

**Prerequisite:** Previous phase P00 passed and has fresh gate receipt.

## Objective

Complete RequestVote and role transitions inside `raft-core` and simulation. Treat durable intent as an effect barrier already in core even though P03 adds physical WAL.

## Allowed deliverables

- `crates/raft-core/src/*`, related protocol structs if needed, `crates/raft-sim` election fixtures.
- Updated `docs/API_CONTRACT.md` on exact Event/Effect declarations.

## Ordered execution steps

1. Implement Role Follower/Candidate/Leader transitions and randomized election timeouts via supplied simulation seed/runtime-generated durations. Use monotonically increasing timer generation to ignore stale timeouts.
2. On Candidate timeout: term increment, vote self, emit `PersistTxn(term,vote)` before requesting votes (dependency barrier). Do not count self vote as durable until storage completion.
3. On RequestVote: stale term reject; higher term durable update and stepdown; enforce votedFor uniqueness and candidate lastLogTerm then lastLogIndex freshness ordering. Grant vote only with durable barrier.
4. On responses: ignore stale term/rpc id, step down on higher term, become Leader only with 2/3 votes. Initialize next/match as Raft Figure 2; no illegal same-term double leader.
5. On receiving same-term valid AppendEntries as Candidate, yield to Follower; stale AppendEntries never extends election timer.
6. Implement immediate current-term NoOp append intent on becoming Leader, but leave full replication to P02; persist intent before sending related RPC.

## Mandatory real Rust tests

- `single_leader_per_term`: directed split-vote and 100 seeded race traces never give two quorum winners in one term.
- `stale_candidate_rejected`: latest term/index ordering for RequestVote, including longer old-term Log.
- `vote_persist_intent_before_reply`: effect order ensures grant and self-vote RPCs depend on durable transaction.

## Boundary / acceptance

No TCP yet; first leader election verified only in actual core used by sim. The test may inspect durable-intent effect ordering without asserting physical fsync (P03 handles that).

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
python3 scripts/verify_gate.py 01
```

## Required evidence

- Every Rust test ID and exact pytest node ID listed in `gates.json` for this stage exists and passes.
- Previous stages remain green. All new transitions and persistence boundaries have structured trace or assertions.
- Codex final output reports actual commands and results, not forecasts.
