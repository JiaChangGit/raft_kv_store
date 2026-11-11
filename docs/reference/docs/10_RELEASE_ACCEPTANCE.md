# 10. Final Go / No-Go Acceptance

Release is **NO-GO** unless every gate P00..P08 passed **on the user's WSL2 machine** and reviewer inspected the evidence.

## Release Checklist

- [ ] `cargo fmt --all -- --check` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes
- [ ] `cargo test --workspace` passes with discovered required named tests
- [ ] Python three-process integration tests pass
- [ ] deterministic seeded simulation reproducible (>=1000 seeds as stress target)
- [ ] no violations of Figure 3 invariants in recorded runs
- [ ] stale/partitioned leader cannot return successful new PUT/GET
- [ ] fsync/commit/reply crash boundaries tested, no early ACK
- [ ] RequestVote durability under restart verified
- [ ] lagging/divergent follower repairs uncommitted suffix
- [ ] same client request retry has one state machine effect and stable response
- [ ] linearizability checker tested against intentionally invalid history
- [ ] benchmark and failover numbers recorded with environment, sample count and raw results
- [ ] architecture ADR, paper mapping and failure analysis updated
- [ ] reproduction command + seed + fault schedule provided for failures

## Known limits that MUST remain documented

3 fixed nodes tolerate at most one unavailable node while continuing commits; no snapshot/compaction, dynamic membership, TLS, byzantine protection, disk corruption guarantees or universal power-loss durability promise. Unstable network can prevent liveness. Benchmarks of WSL2 single machine are not network-distributed performance predictions.

## Example evidence layout after implementation

```
.gates/phase-00.json ... phase-08.json
artifacts/codex/phase-00.log ...
artifacts/reports/sim-seeds.json
artifacts/reports/crash-boundaries.json
artifacts/reports/linearizability.json
artifacts/reports/benchmark.json
artifacts/traces/<seed-or-run-id>/node-{1,2,3}.jsonl
```

Evidence files above are **future output expectations**, not files currently provided or proven to exist.
