# 03. RPC Protocol & Persistence Format v1

## 1. Wire Protocol

每個 TCP frame 為 `u32_be payload_length` + UTF-8 JSON message；最大 Payload **1 MiB**，收到超限長度必須先拒絕、不得配置相同大小 buffer。Receiver 支援 TCP partial reads/writes；單一 stream 永遠以 frame 解析，proxy 故障注入僅操作完整 frame。

Envelope example（欄位定義，不等於必須逐字格式）：

```json
{"version":1,"cluster_id":"test-raft-1","from":1,"to":2,"rpc_id":42,"kind":"AppendEntries","body":{}}
```

- `cluster_id` 與 node 身分和 target 不符：拒絕，不能被視為合法 vote/ack。
- `rpc_id` 只用於 transport correlation；不能代替 Term 或 log-index 驗證。
- `version` 不符返回 protocol error；未知欄位策略固定且測試涵蓋。
- JSON payload 做 bounded deserialization、bounded strings/array；Key<=256 bytes、Value<=4096 bytes；Entries per AE<=32 初始值。

## 2. Raft RPC

### RequestVote

`Request{term:u64,candidate_id:u8,last_log_index:u64,last_log_term:u64}`

`Response{term:u64,vote_granted:bool}`

拒絕條件：request term 小於 currentTerm、已投給其他 candidate、或 Candidate Log 不夠新。比較 Log 新舊時先比 last_log_term，再比 last_log_index。對更高 term 必須同步持久化新 term、清空投票後才送出 response。投票成功時先 fsync 候選 node id。

### AppendEntries

`Request{term:u64,leader_id:u8,prev_log_index:u64,prev_log_term:u64,entries:[LogEntry],leader_commit:u64}`

`Response{term:u64,success:bool,matched_through?:u64,conflict_index?:u64,conflict_term?:u64}`

先檢查 term，`prevLogIndex/Term`；若衝突，針對本次新 Entries 的 suffix 進行 atomic truncate+append transaction。沒有修改也可回覆 Success，但 `matched_through` 必須是本次 request 實際驗證的最高 Prefix，不能報本地未知之 suffix。空 heartbeat success 可以確認 prevLogIndex；Follower 不得 Apply 不匹配 suffix。

**期初**可以只遞減 nextIndex 重試，`conflictTerm/conflictIndex` 為可選後續優化；不改安全性。

## 3. Client RPC

```
ClientRequest {version, client_id:u128, sequence:u64,
               operation: Put(key,value) | Delete(key) | Get(key)}
ClientResponse{request_id, status: OK|NOT_LEADER|TIMEOUT_UNKNOWN|
                     REQUEST_ID_REUSE|INVALID_ARGUMENT|STORAGE_ERROR,
               result?, leader_hint?, committed_index?}
```

NoOp 只能由 Raft Leader 產生、不能由 Client 注入。對同一 `(client_id,sequence)` 的 Command 內容必須一致。V1 同 Client 一次只有一個 logical outstanding operation；跨 leader 新請求靠 canonical command equality + deterministic session table。

## 4. Durable WAL Transaction Format

V1 binary record **建議**明確定義以下欄位（可在 P03 固定 offset，落實 round-trip tests）：

```
record = magic[4] | version:u16_le | record_type:u16_le |
         payload_len:u32_le | sequence:u64_le | payload:bytes |
         crc32c:u32_le
```

- `magic` 例如 `RFT1`；`version=1`；CRC32C 覆蓋 header + payload，不包含 checksum 自己。
- Record 必須以 deterministic encoding 定義 `HardStateUpdate{term,voted_for}`、`LogTxn{truncate_from?:index,append:[LogEntry]}`；允許 `Txn{hard_state?,truncate_from?,append?}` 一次表達多項變更。
- 在單一 record 同時含 truncate + append，避免重啟看到一半套用的奇怪中間狀態。
- 所有 record append 至 WAL，成功 `File::sync_all()` 才可對依賴者回 success；每次 fsync 不代表只能寫一個 entry，可 batch 但 V1 建議同步簡單優先。
- 初始檔案建立/rename 如採用原子更新 metadata，需同步父目錄 metadata；絕不假設有檔案卻必然已 durable。
- `node.meta` 固定包含 cluster_id,node_id,format_version；若 identity 衝突 Fail Closed。

### WAL Crash Recovery

1. 從頭掃描 Record：檢查 Magic、Version、長度上限、Sequence Monotonicity 與 CRC。
2. 完整有效的交易按順序 Replay 到 term/vote/log（**不是 KV**）。
3. **只在可確定的 incomplete trailing record** 容許截斷恢復；中間損壞 / 完整長度卻 CRC 錯誤一律 Fail Closed。
4. WAL 可以有未經 fsync 卻在 crash 後完整保留下來的交易；這是允許的，只要已 ACK 交易絕不遺失。
5. 重建 role=Follower、commitIndex=0、lastApplied=0、KV/Session empty；藉由後續合法 consensus knowledge 重新 Apply 已確認提交 prefix。
6. 在 Recovery 後重新啟動 IO / Timer；若 storage IO error 應保持不可用直到人工介入。

## 5. Durability / Visibility Contract

| Action | 必要 barrier |
|---|---|
| RequestVote grant | 更新 term/voted_for 已成功 fsync |
| Candidate 發出 RequestVote | 更新 term、自投票已成功 fsync |
| AppendEntries success with mutation | truncate/append 已 fsync |
| Leader 本地將自己列入 durable match | entry 已 fsync |
| 新 Term 之 stepdown response | 更高 term 已 fsync |
| Client success | entry 在合法 Commit Index 中，且本機 state machine 已 Apply |
| Apply | Index 已確認 Commit；不可直接依 WAL length Apply |

## 6. Explicit failures

- `fsync` / write failure：不可回 durable success，關閉不安全服務、發 trace，test 驗證。
- Malformed record：fail closed，不能跳過造成 index hole。
- Partial TCP send：繼續傳剩餘 bytes 或中止連線；不能把半個 frame 當成完整成功 RPC。
- Disk space exhaustion：停止接受新寫入並報錯，不能用 RAM-only commit fallback。

## 7. 需留待 P03 固定的細節

明確列出 Byte Offset、CRC32C Polynomial 及 Test Vector、截斷尾部原子性、`node.meta` 建立同步步驟、最大 WAL Record 長度。任何変更要更新文件、測試與版本，不能無聲改格式。
