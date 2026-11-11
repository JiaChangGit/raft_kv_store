# Core / executor 契約 v1

來源：原始 ALL_IN_ONE_SPEC.md 的 Architecture RFC、Protocol、P00～P02。
P00 的 Core 暫不實作共識轉移；以下固定後續實作介面，不代表已實測。

## 固定設定

Node ID 僅 1、2、3；quorum 是兩個不同合法 ID。index 0 是不落盤的 term 0
sentinel，真實 entry 自 1 連續增加。Command 是 NoOp 或具 client/sequence 的
Put、Delete、Get。Core 不呼叫 socket、檔案系統或主機時鐘。

## 事件與有序效果

`Core::new(id: NodeId, state: DurableState) -> Result<Core, String>` 從持久狀態恢復，
總是 Follower、commit_index=last_applied=0。`Core::step(Event) -> Vec<Effect>`
接受 election/heartbeat generation、Envelope 或 ClientRequest。效果依序包含
Persist、Send、Apply、ResetElection、ResetHeartbeat、Reject。

`Effect::Persist { barrier_id: u64, state: DurableState }` 表達必須同步保存的狀態；
storage 將它編為 append-only transaction，不代表每次重寫 WAL。該效果在同次
有序清單最前面。單一 executor 先 write + sync_all 成功，再呼叫
`Core::persisted(barrier_id) -> Vec<Effect>`；完成後才可執行原清單後續效果。
completion 所產生的 Apply 仍須先通過 current-term durable quorum 條件。

persisted 必須核對 pending ID；只有完成的狀態可增加 durable_index 與 leader
self-match。Core 在 pending barrier 期間不接受下一個 consensus event。executor
不得把尚未完成的內部變更當作 durable 證據。同步失敗呼叫 storage_failed，停止
整份剩餘清單並 fail closed；不得回 ACK、成功 client reply 或繼續事件處理。

`Effect::Apply(LogEntry)` 僅代表有序 committed entry；client 成功由 runtime 在
本地 KV apply 完成後回覆。新 GET 與 duplicate retry 都經安全的共識路徑。

## 計時、網路與模擬

runtime/simulator 收到 ResetElection 後，各自以 injected randomness 選 400～800ms；
heartbeat 初始 100ms。generation 讓過期 tick 失效。模擬排程嚴格按
`(virtual_time, sequence)`，sequence 解決同時間排序；seed/input 決定完整 trace。
網路 drop 不投遞；delay 重新排程；duplicate 用新的 sequence 排程兩份 frame。

每 peer 一個有效 AppendEntries correlation，記錄 rpc_id/term/leader epoch/
prev index/count；逾時後以新 ID 淘汰舊 correlation，舊回覆不更新 match。
MockDisk 分離 volatile/durable，未 sync record 在 crash 後允許存活或遺失；
已成功 sync 的狀態不得遺失。正式 runtime 與模擬器呼叫同一 Core。

runtime 的 outbound connect/write/read 共用總 I/O deadline，partial read 不會重設整筆
期限。連線工作額滿或立即連線失敗時，仍等原期限才送 core timeout，避免重試忙迴圈。
這只影響傳輸與排程，不把 transport timeout 當成成功 ACK 或改變 commit predicate。
