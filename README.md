# Rust Raft KV Store

[中文版](README.zh-TW.md)

A key-value store replicated with Raft, written from scratch in Rust as a learning and portfolio project.
It runs one Raft group of up to seven nodes, changes membership one server at a time, takes snapshots and compacts its log, serves linearizable reads through ReadIndex, and batches writes with group commit.
The same `raft-core` runs inside a deterministic simulator and inside the real TCP nodes.

Most of the code, tests and documents were written by AI models under the author's direction; [how that worked](#how-this-was-built-with-ai) is described below, including its limits.
Most documents under `docs/` are in Traditional Chinese.

## What it does, stage by stage

| Stage | Added | Specification |
|---|---|---|
| V1 | Three fixed nodes: election, replication, commit, a write-ahead log, crash recovery, client deduplication, a deterministic simulator, fault-injecting proxies | [V1 report](docs/IMPLEMENTATION_REPORT.md) (historical) |
| S1 | WAL format 2: checksummed record headers, a file header, safe handling of a torn tail | [STORAGE_FORMAT_V2](docs/STORAGE_FORMAT_V2.md) |
| S2 | Snapshots, log compaction, chunked InstallSnapshot | [SNAPSHOT_COMPACTION](docs/SNAPSHOT_COMPACTION.md) |
| S3 | ReadIndex: a GET no longer enters the log | [READ_INDEX](docs/READ_INDEX.md) |
| S4 | Membership changes one server at a time; node IDs 1–7; the configuration lives in the log | [MEMBERSHIP](docs/MEMBERSHIP.md) |
| S5–S6 | Group commit, batched replication, writing only what changed; fixes from an independent review | [PERFORMANCE](docs/PERFORMANCE.md) |
| S7–S8 | Studies of the AI framework used to build this (not part of the store) | [framework-study/](framework-study/README.md) |
| S9 | This public clean-up | [PUBLIC_RELEASE](docs/PUBLIC_RELEASE.md) |

## Architecture

```mermaid
---
config:
  look: neo
---
flowchart LR
    C["client tools"] -->|"TCP frame"| N["raft-node"]
    N -->|"Event"| R["raft-core"]
    R -->|"Effect"| N
    N -->|"save_delta"| S["raft-storage"]
    N -->|"apply"| K["kv-state"]
    N <-->|"frame"| T["transport"]
    M["raft-sim"] -->|"same Core"| R
    X["cluster.py"] -->|"processes, faults"| N
    R -.->|"types"| P["protocol"]
```

`raft-core` is pure: it takes an `Event` and returns ordered `Effect`s, and never touches the clock, the network or the disk.
In `raft-node`, one actor thread owns the core, the WAL and the state machine; other threads only move frames and events.
A write follows this path on the leader:

```mermaid
---
config:
  look: neo
---
sequenceDiagram
    participant C as client
    participant A as leader actor
    participant R as leader core
    participant W as leader WAL
    participant F as follower
    participant K as KV
    C->>A: PUT frame
    A->>R: ProposeBatch
    R-->>A: Persist, then Send
    A->>W: save_delta
    W-->>A: synced
    A->>R: persisted
    A->>F: AppendEntries
    Note over F: syncs before replying
    F-->>A: AppendResponse
    A->>R: Message
    R-->>A: Apply
    A->>K: apply
    A-->>C: OK and old value
```

[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) has four diagrams (components, a write, a ReadIndex read, adding a member), each with the files and functions it was traced from.

## Quick start

You need Linux (developed on WSL2 Ubuntu, with data on ext4), Rust through rustup (the version is pinned in `rust-toolchain.toml`) and Python 3. Run everything from the repository root.

```bash
python3 -m venv .venv
.venv/bin/python -m pip install -r requirements-dev.txt
cargo build --workspace --locked
```

Start three nodes, each in its own terminal. Only node 1 bootstraps, and only once for the whole cluster; the others start empty.

```bash
cargo run -p raft-node -- --id 1 --config config/cluster.toml --data-dir data/node-1 --bootstrap
cargo run -p raft-node -- --id 2 --config config/cluster.toml --data-dir data/node-2
cargo run -p raft-node -- --id 3 --config config/cluster.toml --data-dir data/node-3
```

From another terminal, add nodes 2 and 3 to the configuration, then write and read a key:

```bash
.venv/bin/python tools/kv_admin.py --config config/cluster.toml add 2
.venv/bin/python tools/kv_admin.py --config config/cluster.toml add 3
.venv/bin/python -m tools.kv_client --address 127.0.0.1:7201 --address 127.0.0.1:7202 --address 127.0.0.1:7203 --client-id 7 --sequence 0 Put x A
.venv/bin/python -m tools.kv_client --address 127.0.0.1:7201 --address 127.0.0.1:7202 --address 127.0.0.1:7203 --client-id 7 --sequence 1 Get x
```

A client sends one request at a time. After `TIMEOUT_UNKNOWN`, retry with the same client ID, sequence and content; a new request uses the next sequence. Put and Delete return the previous value.
`tools/kv_admin.py ... status` shows each node, and `remove N` takes a node out. To replace a failed node, add the new one first, then remove the old one.
Do not re-add a node ID with a wiped data directory and do not bootstrap a second node: this version does not detect either mistake, and both can break safety.
[docs/HANDS_ON.md](docs/HANDS_ON.md) walks through an election, a write, killing the leader and restarting it (in Chinese).

## Verification

| What | How |
|---|---|
| Unit and integration tests | `cargo test --workspace`, `.venv/bin/python -m pytest -q tests` |
| Lints | `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` |
| Simulation | 1,000 seeds × 10,000 events per seed, with independent invariant observers (`raft-sim`) |
| Crashes | failpoints at the write, sync, acknowledge, commit, apply and reply boundaries; real processes killed with SIGKILL and restarted on the same data |
| Network faults | proxies that drop, delay, duplicate and reorder whole frames; histories checked for linearizability |
| Mutation testing | 63 planted bugs, each run in an isolated copy; every one must make a test fail |
| Acceptance contract | 15 phases (P00–P08, P10–P15) in `acceptance/final.json`, run by `scripts/verify_gate.py` |

`.venv/bin/python scripts/status.py` shows the latest result of each phase and whether it matches the current source.
`bash scripts/final_acceptance.sh` reruns everything from the mutation tests to P15; it can resume after an interruption and stops if a source file changes. It took about 45 minutes on the author's machine.
The tag `s9-public-verified` is created only after every phase passed on the exact source of that commit. Receipts and reports are written to `artifacts/`, which is not under version control.

## What is not verified

- There is no formal proof, of Raft or of this code.
- There is no long soak test. Real-process linearizability checks look at no more than 24 operations at a time.
- The simulator's administrator sends one membership change at a time, and each seed has 64 client operations.
- S1 was reviewed only by another session of the same model, not by a different model.
- The bug lessons are drafts: nobody else has followed them yet.
- SIGKILL is not a power failure; a successful `sync_all` is trusted to mean the data is on disk.
- Membership operator mistakes (re-adding a wiped node, bootstrapping twice) are not detected.
- Performance numbers come from one WSL machine and say nothing about other hardware.
- The diagrams were checked only as Mermaid source; no renderer works on the author's machine.

## Documents

[docs/README.md](docs/README.md) sorts every document into current guides, specifications, records and historical versions.
Start with [docs/LEARNING_GUIDE.md](docs/LEARNING_GUIDE.md) (one PUT through the whole system) or the [bug lessons](docs/lessons/README.md).
Records keep the paths of the author's machine as they were written; `CLAUDE.md`, `AGENTS.md` and `.claude/` are the author's local setup for AI tools.

## How this was built with AI

The author set the goals, approved a specification before every stage, spot-checked results and kept every outward action (push, pull requests, deployment) to themselves. AI models wrote the specifications, code, tests, lessons and reports.
Every stage followed the same order: specification, approval, failing tests, implementation, simulation and fault tests, independent review, lessons, acceptance, then a local commit and tag.
Three independent reviews, each by a model other than the one that wrote most of the reviewed work, found real defects: a recovery bug, a weakened simulator check and a boundary bug in S1–S4; in S5, an acceptance phase that had never finished although the documents called it done; in the framework changes, a usage counter that could print arbitrary words from private transcripts.
The limits are real too: the same model often wrote and graded, experiments ran each condition only once or twice, and the sessions whose start directory was recorded all started one level above the project, so the project's permission rules were never loaded and "read-only" and "no push" depended on the model's own restraint.
[docs/AI_COLLABORATION.md](docs/AI_COLLABORATION.md) has the details (in Chinese), and [framework-study/](framework-study/README.md) has the study of the AI framework itself.

## Licence

MIT; see [LICENSE](LICENSE).
