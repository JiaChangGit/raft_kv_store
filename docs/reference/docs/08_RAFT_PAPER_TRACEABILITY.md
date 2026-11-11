# 08. Raft Paper and Codex Reference Traceability

## Primary authority

**Diego Ongaro and John Ousterhout, “In Search of an Understandable Consensus Algorithm (Extended Version)”**, May 20, 2014, https://raft.github.io/raft.pdf

| Paper section | Required project behavior | Gate |
|---|---|---|
| Figure 2, Persistent State | term, vote, log persisted before dependent RPC response | P01/P03 |
| Figure 2, RequestVote | term check and log freshness check | P01 |
| Figure 2, AppendEntries | `prevLog` check, conflicting suffix, commitBound | P02/P03 |
| Figure 2, Leaders | current-term commit predicate, nextIndex/matchIndex | P02 |
| Figure 3 | Election Safety, Leader Append-Only, Log Matching, Leader Completeness, State Machine Safety | P01/P02/P07 |
| §5.2 | randomized election, split vote, one vote per term | P01 |
| §5.3 | log replication and conflict repair | P02 |
| §5.4.1 | Election Restriction / up-to-date candidate | P01 |
| §5.4.2 / Figure 8 | cannot directly commit previous-term entry by counting replicas | P02 |
| §8 | client retry & read-only consistency safety | P05/P07 |

Important nuance: Figure 8 uses five nodes as a pedagogical counterexample. V1 runtime remains three processes. Implement current-term predicate regardless; explain paper's full counterexample in tests/docs, and optionally use a pure isolated 5-voter test fixture (not a production 5-process deployment) to demonstrate it.

## Supplementary authoritative reference

Diego Ongaro, *Consensus: Bridging Theory and Practice* (dissertation), https://ongardie.net/var/blurbs/pubs/dissertation.pdf . Read-only queries require current-term committed entry, quorum confirmation and applied state when bypassing the log. V1 deliberately **does not bypass the log**.

## Codex CLI reference

- CLI and `codex exec`: https://learn.chatgpt.com/docs/cli/reference
- Non-interactive execution: https://learn.chatgpt.com/docs/non-interactive-mode
- CLI quickstart: https://learn.chatgpt.com/docs/cli

The helper script uses `codex exec --sandbox workspace-write --cd <repository> -` and avoids unrestricted sandbox bypass. Verify locally using `codex exec --help`; CLI versions can change.
