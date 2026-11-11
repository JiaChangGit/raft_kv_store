# 02. Architecture RFC — Fixed 3-Node Raft KV

## 1. 部署（唯一正式拓撲）

```
Clients (Python) --> Node RPC endpoint (7201/7202/7203)
                                   |
          +------------------------+-------------------------+
          |                        |                         |
       Node 1                   Node 2                    Node 3
   raft TCP 7101             raft TCP 7102              raft TCP 7103
   Raft actor                Raft actor                 Raft actor
   KV applied state          KV applied state           KV applied state
   WAL data/node-1           WAL data/node-2            WAL data/node-3
          |                        |                         |
          +-- directional framed RPC fault proxies ----------+
```

三個 `raft-node` 是不同 Linux Process。Config 固定 cluster ID、node IDs、peer address、client address、WAL directory；每 Node 的 data directory 不得交換。**測試環境**的 peer-addr 為 Python proxy endpoint，以利注入故障；非測試環境可直連 TCP。

## 2. Cargo Workspace 與責任

| Package | 責任 / 不可做 |
|---|---|
| `raft-protocol` | `NodeId`, `Term`, `Index`, `Command`, RequestVote, AppendEntries, Client envelope, encoding 與 bounds；不得內嵌 Raft algorithm |
| `raft-core` | 完整 Election、Log replication、Commit predicate、Event→Effects；不直接做 socket/fs/cargo runtime |
| `raft-sim` | Mock durable storage, virtual time, deterministic queue, independent oracle；絕不 fork consensus algorithm |
| `raft-storage` | durable transaction WAL/CRC/fsync/recovery；不決定 commit |
| `kv-state` | deterministic Put/Delete/Get/NoOp + SessionTable + result；絕不修改 Raft commit |
| `raft-transport` | TCP framing, reconnection, rpc_id, queue, request timeout；不修改 term/vote/log |
| `raft-node` | Single-owner Raft runtime、WAL effect barrier、CLI/config、endpoint、node process |
| `tests/integration` | Python subprocess + framed fault proxies + history/checkers |

## 3. Raft Core 契約

```
step(event: Event) -> OrderedEffects

Event (examples)
  ElectionTimeout{generation}
  HeartbeatTick{generation}
  RequestVoteReceived{peer, request, rpc_id}
  RequestVoteResponse{peer, response, rpc_id}
  AppendEntriesReceived{peer, request, rpc_id}
  AppendEntriesResponse{peer, response, rpc_id}
  ClientCommandReceived{request, connection_handle}
  StorageFailure{error}     [runtime fatal]

OrderedEffects (examples)
  PersistTxn{transaction, barrier_id}
  SendRpc{peer, frame, depends_on_barrier?}
  ReplyRpc{peer, rpc_id, result, depends_on_barrier?}
  ApplyUpTo{index, depends_on_barrier?}
  ReplyClient{token, result, depends_on_commit_apply}
  ScheduleTimer{kind,generation,duration}
  EmitTrace{fields}
```

可以在 Rust 中使用 enum / structs 定義；不要讓 Core 呼叫 OS 時鐘或阻塞 Socket。同步 V1 runtime 的最簡單規則：`PersistTxn` 順序執行並完成 `sync_all()`，只有成功後才執行依賴該交易的 Send/Reply/Apply；同步失敗直接停止不安全操作，不處理新 Consensus Events。不得讓 in-memory mutation 與未完成 durable mutation 在下一事件中混淆。

**注意**：從 `step` 返回後的 `RaftState` 可能暫時包含未 durable 的變更；V1 用 *synchronous barrier before processing next event* 確保其不被其他事件看見。由 runtime 觀測的 local match 必須代表 durable；不可讓 Commit Logic 把待 fsync 的 Entry 當作已持久保存。

## 4. Role 與 Leader 變更

- Restart ⇒ Follower。Follower 超時後增加 Term、自投票，先持久化，再發送 RequestVote。
- Candidate 得到 2 票成為 Leader，初始化 peer nextIndex、matchIndex，append current-term NoOp，持久化後複製。
- 所有 Node 收到較高 term，持久化新 term 與 `votedFor=None` 後按規則退回 Follower；僅 current term 的 responses 可被當作有效證據。
- 同 term 有效 AppendEntries 可使 Candidate 退回 Follower；stale-term message 不得 reset timer。
- Election timeout 隨機 400–800 ms；Heartbeat 100 ms；所有 timer 含 generation 以忽略過期 tick。只要有有效 leader AE 或給予票才重設 Follower election timer。

## 5. Log Invariants 與索引

- Index 0 是虛擬 sentinel (`term=0`)，不落 WAL；真實 entries 從 index 1。
- Log 無 gaps。`nextIndex` 是下一個要發送之 index；`matchIndex` 是經確認匹配且 durable 的最高 index。
- 一個 Peer 最多一個 outstanding AppendEntries，metadata 包含 `rpc_id, term, leader_epoch, prevLogIndex, entry_count`。
- Success 只能確認 `prevLogIndex + entries.len()`（含空 heartbeat 確認的 prev index），不能信任 peer 的 lastLogIndex 直接推高 matchIndex。
- Failure 僅對當前合法 Request 調整 nextIndex，不容許 stale failures 倒退已驗證 Prefix。
- 只有與新 entries 相同 index、不同 term 的衝突才 truncate 自該 index 起的 suffix；相同 term 的 entries 不覆寫。
- Follower `commitIndex = min(leaderCommit, confirmed_matched_prefix_end)`，不得假設 follower 多出未驗證 suffix 就可 apply。

## 6. Leader Commit

```
for N descending from locally_durable_last_index to commitIndex+1:
    if log[N].term == currentTerm and durable_match_count(N) >= 2:
        commitIndex = N
        break
```

`durable_match_count` 計 Leader 自己（僅 fsync 成功後）及至少一個 Follower。當前 Term Entry 提交後，較早合法 prefix **間接 committed**。已提交 log 不刪改；Apply 遵守 `lastApplied <= commitIndex` 及 index 順序。

## 7. GET 與 Client

- V1 `Get` 也是 Command log entry，每個請求在 log 的線性化點是其 Commit；結果由 Apply 時計算。
- 所有成功回覆發生在本地 Apply 後。重試相同 request id 的舊結果只透過安全的 committed-apply path 回覆，不能只靠過時 Leader RAM Session。
- Client 在非 Leader 得到 `NOT_LEADER(leader_hint?)`；不可保證 hint 永遠正確。
- 超時必須標成 `UNKNOWN_OUTCOME`。Client 使用同一 `(client_id,sequence)` 重試，不生成新的 sequence。

## 8. State、Election、Dataflow、Recovery 圖

- `diagrams/raft_states.mmd`
- `diagrams/election.mmd`
- `diagrams/replication.mmd`
- `diagrams/recovery.mmd`
- `diagrams/modules.mmd`

這些 Mermaid 原始碼可以直接在 Markdown Preview / Mermaid Live Editor 查看，不影響可執行測試。
