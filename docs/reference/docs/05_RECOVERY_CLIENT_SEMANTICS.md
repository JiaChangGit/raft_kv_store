# 05. Recovery, Replicated Read, Retry & Session Semantics

## 1. Restart Sequence

```
stop/restart -> verify node.meta identity
             -> scan/check WAL txns
             -> restore durable Term/Vote/Log only
             -> reset role=Follower, commit=0, lastApplied=0, KV=empty, Session=empty
             -> listen peers / restart election timer
             -> receive legitimate leader AE with commit info OR elect leader and commit new-term NoOp
             -> apply only confirmed prefix in index order
             -> serve successful client requests only on safe Leader commit/apply path
```

**嚴禁**把 WAL Log 本身當成所有 Entries 已經 Commit 的證據。新 Leader 應先 append 本 Term NoOp，持久化及提交之後才可推進早期合法 Prefix 的確認。Follower 的 `leaderCommit` 通知也必須綁定當次已匹配的 Prefix。

## 2. Session Model

- ClientId: stable random u128（由 Client 生成）；Sequence: 0..n 嚴格遞增；**同 client 僅一筆 outstanding logical request**。
- Session table key 為 `(client_id, sequence)`，V1 保存完整歷史，不做 session GC（避免先驗證不了舊 request）。Record 存 canonical command bytes 或其強校驗 digest + 原始 deterministic result。
- 每個 Replicated `Put/Delete/Get` 對應 ClientId/Sequence；`NoOp` 不含 ClientId。
- Apply 首次看到 request：先以已 committed map 狀態執行，保存 result，再回傳。
- Apply duplicate 且 command 相同：無副作用，返回原結果；若 command 不同：`REQUEST_ID_REUSE`，不修改 KV。
- 重試在不同 Leader 可能形成**多筆 Log Entries**，但 Session Table 應保證同一 logical request **只 Apply 一次效果**。這不是「exactly one log entry」。
- 若 Client 將舊 request ID 用作新的邏輯讀取，返回舊結果是 duplicate semantics，不可宣稱是新的線性化 read；新 logical GET 必須分配新 sequence。

## 3. GET Linearizability

V1 每個新 GET 都進入 Log。Leader 必須能 Commit/Apply 該 GET entry 才返回值，這讓線性化點落在該 entry 的 Commit / 序列順序。舊 Leader 無 Quorum 無法提交新 GET。任何 duplicate result 回覆也不能繞過 Leader safe path，避免 partitioned leader 的快取結果被誤當成新讀取。

**重要區別**：Log append 本身不是線性化點；Client 成功只在 committed apply 後，已滿足 real-time ordering（在單 client 一個 pending logical op 的限制下）。

## 4. Timeout / Uncertain Write

```
PUT(k=v, id=21) -> quorum persisted -> leader crash BEFORE reply
Client timeout -> UNKNOWN_OUTCOME
Client retries SAME id=21 -> committed state replay -> same original result
```

相同 ID 不重複副作用。不同 ID 的新 PUT 則是新的合法操作（可能再覆寫 KV）；不可以把原本 unknown write 直接換 ID 重送。

## 5. Client Error Contract

| Status | Meaning |
|---|---|
| OK | Leader 已合法 Commit 並 Apply 對應 Command；回傳 deterministic result |
| NOT_LEADER | Node 不接受此新操作；可提供非權威的 leader hint |
| TIMEOUT_UNKNOWN | Request 結果未知；以原 ID 重試 |
| REQUEST_ID_REUSE | 相同 ClientId/Seq 但不同內容 |
| INVALID_ARGUMENT | Command 尺寸、型別、key 等不合約 |
| STORAGE_ERROR | Node 無法提供 durability guarantee；不回成功 |

## 6. V1 Non-goals

ReadIndex、Leader Lease、Follower Stale Read、Snapshot、Session TTL/GC、並發同一 Client 多 request；這些都會增加 correctness proof burden，不得偷偷加進 V1。
