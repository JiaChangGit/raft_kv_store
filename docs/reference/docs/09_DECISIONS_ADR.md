# 09. Architecture Decision Records

- **ADR-001 / Accepted**: Rust stable over C++. Justification: fewer memory/concurrency incidental hazards; consensus safety still needs independent verification.
- **ADR-002 / Accepted**: Single owner actor for Raft, multithreaded I/O through channels. Lower correctness surface than per-RPC shared mutex mutation.
- **ADR-003 / Accepted**: Sync WAL durable barrier in event loop for V1. Prioritize correct persistent ACK over throughput.
- **ADR-004 / Accepted**: Single in-flight AppendEntries per peer for V1. Easier stale response handling; pipeline optimization later.
- **ADR-005 / Accepted**: Replicated GET; read result produced by committed-apply sequence. ReadIndex optimization deferred.
- **ADR-006 / Accepted**: Full session history in memory, reconstructed from committed log. No session garbage collection until snapshot design.
- **ADR-007 / Accepted**: Volatile commitIndex/lastApplied on restart; committed-knowledge relearned. No premature replay of WAL entries.
- **ADR-008 / Accepted**: Versioned WAL records with CRC and explicit truncate+append transaction; fail closed on ambiguous corruption.
- **ADR-009 / Accepted**: JSON framed TCP with 1MiB frame cap; protocol parsing bounded. Binary codec deferred.
- **ADR-010 / Accepted**: Virtual-time seeded simulator using exact production core. No duplicate algorithm.
- **ADR-011 / Accepted**: Directional frame-level proxy for real-process faults; no direct test bypass.
- **ADR-012 / Accepted**: Fixed 3-node single group; no Dynamic Membership, Snapshot, Multi-Raft, Sharding, TLS, UI.

Any change to an Accepted ADR requires explanation, tests and design review. It must not be an opportunistic shortcut for bypassing a failing acceptance Gate.
