# P02 — Log Replication, Conflict Repair & Commit

**Execute:** `bash scripts/run_phase.sh 02`

**Prerequisite:** Previous phase P01 passed and has fresh gate receipt.

## Objective

Implement full AppendEntries receiver, leader per-peer replication index management, current-term-only direct commit, and ordered Apply intents. This is the central Consensus Safety Gate.

## Allowed deliverables

- `raft-core` replication/commit logic, `raft-protocol` AE fields and `raft-sim` directed tests.
- `docs/COMMIT_RULE_REVIEW.md` with Figure 8 discussion and commit predicate.

## Ordered execution steps

1. Define log sentinel index 0; implement checks for leader term, prevLogIndex existence and prevLogTerm matching.
2. Follower: detect conflict only against new entries, atomic `truncate_from+append` transaction intent, no overwrite of matching entries. `leaderCommit` advances only to prefix confirmed by **this** AE; never apply follower's unmatched suffix.
3. Leader: `nextIndex` init lastIndex+1; self `matchIndex` includes durable entries only; peer `matchIndex` begins 0. One in-flight AE/peer; verify `rpc_id`, term and leader epoch, ignore stale replies.
4. Success reply updates peer confirmed match *at most* `prevLogIndex+entries.len()` (heartbeats can confirm prevLogIndex); do not accept remote reported last index as arbitrary evidence. Failure retries earlier prefix, preserving known match bounds.
5. Compute Leader commitIndex only on current term entry N with 2 durable matches; earlier prefix commits indirectly. Ensure Apply effects follow increasing index and never exceed commitIndex.
6. Add current-term NoOp replication on leader election, currentTerm changes/stepdown while replies are inflight, delayed success/failure scenarios.
7. Include explicit property/test that an old-term entry replicated on majority cannot by that fact alone advance leaderCommit; document 5-node paper Figure 8 counterexample without creating production 5-node deployment.

## Mandatory real Rust tests

- `previous_term_entry_requires_current_term_commit`: negative and positive cases; majority old term alone must fail, current-term NoOp permits prefix commit.
- `conflicting_suffix_repaired`: divergent follower suffix is replaced without deleting committed prefix.
- `append_response_cannot_inflate_match_index`: stale/duplicate/empty heartbeat response cannot claim arbitrary follower last index.

## Boundary / acceptance

All five Figure 3 invariants instrumented in sim; absence of a specific `log[N].term == currentTerm` condition is an **automatic No-Go**.

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
python3 scripts/verify_gate.py 02
```

## Required evidence

- Every Rust test ID and exact pytest node ID listed in `gates.json` for this stage exists and passes.
- Previous stages remain green. All new transitions and persistence boundaries have structured trace or assertions.
- Codex final output reports actual commands and results, not forecasts.
