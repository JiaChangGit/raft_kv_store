# Rust Raft KV Store

Work only in this project. The sibling ai-general-framework checkout is read-only.
Preserve the three original specification Markdown files. Original ZIP/gates/scripts
were not supplied; acceptance/v1.json is an explicitly different acceptance contract.

Implement P00 through P08 in dependency order, with tests and independent review.
Use one actual raft-core in simulator and runtime, fixed IDs 1/2/3 and quorum 2.
No existing Raft library, unsafe Rust, membership changes or leases.
Snapshots follow docs/SNAPSHOT_COMPACTION.md (S2).
One actor owns consensus state. Persist term/vote/log before dependent messages;
storage failures fail closed. Only durable matches and current-term entries advance
leader commit. Recovery restores only durable term/vote/log plus the durable snapshot
(committed entries only) and never applies WAL entries beyond it until relearned.
New GETs use ReadIndex (docs/READ_INDEX.md, S3): only a leader that has committed
an entry of its term and confirmed leadership with a majority round started after
the read arrived may answer, at an applied index >= readIndex; reads never write
the log. Deduplicate writes using replicated per-client latest (client_id,
sequence, canonical command, original result) records; an older sequence is
rejected without effect (S2, D1). GETs are not deduplicated (S3, D3).
All fault-test peer traffic, including reconnects, uses complete-frame proxies.
Never weaken tests or acceptance to make an implementation pass. Record actual
commands, counts, failures, limits and pending work; no formal-correctness claim.
No push, PR or deployment. User authorized local implementation and required tools.

Read .agf/managed/project-rules.md and relevant deployed skills when applicable.

## Final-version roadmap (2026-11-12, user-approved)

The purpose is learning and interview demonstration. docs/ROADMAP.md is the plan;
acceptance/final.json is the active contract and acceptance/v1.json is history.
The V1 rules above stay in force until the stage that changes them begins:

| Rule above | Changes at | Until then |
|---|---|---|
| "No ... snapshot", recovery and deduplication sentences | S2 (approved 2026-11-13) | changed above |
| "No ... ReadIndex", "All new GETs replicate" | S3 (approved 2026-11-13) | changed above |
| "fixed IDs 1/2/3 and quorum 2", "No ... membership changes" | S4 | in force |
| No existing Raft library, no unsafe Rust, no leases | never | in force |

When a stage starts: write its specification first, get the user's approval, then
edit only that row and the matching sentence above, in the same commit as the
specification. Every other rule (persist before dependent effects, fail closed on
storage errors, current-term commit, recovery never applies the whole WAL,
deduplication, complete-frame proxies, never weaken tests) carries over unchanged.
Bugs met along the way become lessons under docs/lessons (see its README).
