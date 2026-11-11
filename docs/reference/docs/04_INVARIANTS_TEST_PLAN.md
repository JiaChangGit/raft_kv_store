# 04. Design Invariants & Verification Plan

## Raft Safety Invariants (paper Figure 3)

I1 **Election Safety**：同 term 最多一個合法 Leader。（只記錄已獲得多數投票者；不可依角色字串的時間切片混淆）。

I2 **Leader Append-Only**：Leader 只在自身 Log 尾端 append，不覆蓋/刪除自己的 Log。

I3 **Log Matching**：若兩個 Node 在同 index 有相同 term，則從 1..index 的 Log prefix 相同。

I4 **Leader Completeness**：已 Commit 的 Entry 一定出現在所有後續合法 Term Leader 的 Log 中。

I5 **State Machine Safety**：任何 Node 於 index i Apply Entry e，所有其他 Node 於 index i Apply 時只能 Apply 同 e。

## Engineering Invariants

- `currentTerm` durable monotonic across restart；同 term 同 Node at most one `votedFor`，允許重覆投給**同**候選人。
- 不存在 `lastApplied > commitIndex`；`lastApplied` 連續增加。
- 持久化 ACK 不能在 fsync 前發出；Leader self-match 只含 durable entries。
- Leader commit N 必須 current-term entry + quorum durable match；commitIndex 不倒退。
- Follower AE Success 只能確認受此次 RPC 驗證的匹配 prefix。
- `log[0]` 是虛擬 Sentinel，index 從 1，沒有 gaps。
- 不向 Client 回覆未 Commit/Apply 的成功；Unknown Outcome 不宣稱失敗。
- 同 `(client_id,seq)` 不執行多次 side effect，且結果可被 committed log replay 重現。
- Read 成功一定有 consensus 線性化點；partitioned old leader 不能本地直接成功回答。

## 測試分層（獨立 oracle）

1. Unit：Term/Vote/Conflict/Commit Predicate，包含 negative tests。
2. Deterministic Virtual Event Simulation：虛擬時計、訊息分割、drop、delay、duplicate、crash、recovery、seed replay。
3. Durable WAL Tests：Record round-trip、CRC、尾端不完整、crash-before-sync 與 ack ordering。
4. TCP Runtime Tests：frame partial read/write、連線重建、rpc correlation。
5. Python 3-Process Integration：SIGKILL + restart、網路故障、查詢結果與 logs。
6. Independent Linearizability Checker：對小型 History 枚舉合法 sequential KV semantics；pending/timeout 需要容許可能完成或未完成。

## 必測 Scenario Matrix

| ID | Scenario | 必須觀測 |
|---|---|---|
| T01 | 3-node initial election | 至多一位合法 Leader，2/3 votes |
| T02 | simultaneous candidates | 不會有雙 Leader / split vote 可重試 |
| T03 | outdated candidate | freshness rule 不給票 |
| T04 | higher term observed | step down + persist term before response |
| T05 | normal Put/Delete/Get | committed-apply and return |
| T06 | leader SIGKILL | majority eventual progress |
| T07 | follower SIGKILL | majority writes continue |
| T08 | minority partition | isolated former leader no successful new op |
| T09 | partition heal | conflict suffix repair & convergence |
| T10 | lagging follower | matchIndex bounds and catchup |
| T11 | prevLogIndex/Term mismatch | rejection, retry, no committed truncation |
| T12 | old term majority | direct old-term commit prohibited |
| T13 | crash before fsync | no acknowledged non-durable mutations |
| T14 | crash after fsync before reply | retry yields same logical result |
| T15 | crash after majority before apply | committed remains safe, unknown client |
| T16 | restart with uncommitted suffix | no premature KV apply |
| T17 | stale/duplicate AE response | cannot inflate matchIndex |
| T18 | stale RequestVote / Heartbeat | no term/regression or spurious timer reset |
| T19 | duplicated client request | single effect, original result |
| T20 | same id different payload | deterministic request ID reuse error |
| T21 | stale leader GET | no stale successful read |
| T22 | WAL truncated tail and corruption | incomplete tail truncate / corruption fail closed |
| T23 | fragmented TCP frames | no parse/alloc bug |
| T24 | disk error, disk full | fail closed, no success ack |
| T25 | all nodes restart with durable logs | safe replay after relearning commit index |
| T26 | randomized 1000 seeds | zero invariant violations, reproducible failures |

## Gate Discovery

`gates.json` 的 `rust_tests` 使用 unique identifier，`verify_gate.py` 會先收集 `cargo test --workspace -- --list` 並確認指定名字存在。這**不取代**測試正確性的人類 code review；禁止用空測試混過 gate。

## Linearizability Oracle

History 具有 `client, request_id, operation, invocation_time, response_time/status, result`，必須考慮 real-time order：A 的成功 response 在 B invocation 前 ⇒ A 在順序中先於 B。並行操作可重排；Unknown outcome 的 pending PUT 可能未執行，也可能已完成，不能一律當失敗。每個操作使用 sequential reference KV model + replicated session semantics；對小歷史做 backtracking；為效能把不同 key 分組需先論證與 Session Semantics 相容。

## Liveness Oracle

在「固定時間內網路穩定、至少 2 nodes 運作、磁碟正常、沒有其他 fault」的條件下量測**最終進展**；這是測試條件下的 liveness 證據，不是 Raft 的固定選舉時間保證。未達進展時保留 logs、fault schedule、seed。

## Failure triage

需保存 per-node structured trace（term、vote、index、rpc_id、durable seq、commitIndex、lastApplied）、WAL dump、client history、fault schedule、seed、首次違規 invariant 與重現指令。
