# Core / executor 契約 v1

來源：原始 ALL_IN_ONE_SPEC.md 的 Architecture RFC、Protocol、P00～P02。
P00 的 Core 暫不實作共識轉移；以下固定後續實作介面，不代表已實測。

## 節點與設定

Node ID 為 1～7（S4 起；V1 為固定的 1、2、3）。誰有投票權由 log 中最新的設定 entry 決定，
log 中沒有時取快照記錄的設定；多數一律是目前設定的多數（[成員變更規格](MEMBERSHIP.md) §5、§9）。
沒有設定的節點不發起選舉。index 0 是不落盤的 term 0 sentinel，真實 entry 自 1 連續增加。
Command 是 NoOp、具 client/sequence 的 Put、Delete、Get，或設定 `Config { voters }`。
Core 不呼叫 socket、檔案系統或主機時鐘。

## 事件與有序效果

`Core::new(id: NodeId, state: DurableState) -> Result<Core, String>` 從持久狀態恢復，
總是 Follower、commit_index=last_applied=0。`Core::step(Event) -> Vec<Effect>`
接受 election/heartbeat generation、Envelope 或 ClientRequest；S2 起另有 Compact，S3 起有 Read／CancelRead，
S4 起有 ChangeMembership／CancelMembership 與 LeaderContactExpired（最小選舉逾時到期）。效果包含
Persist、PersistSnapshot、Send、Apply、ResetElection、ResetHeartbeat、Reject、ReadReady／ReadFailed、
MembershipDone／MembershipFailed。

`Effect::Persist { barrier_id: u64, state: DurableState }` 表達必須同步保存的狀態；
storage 將它編為 append-only transaction，不代表每次重寫 WAL。同次清單中，barrier 排在所有依賴它的效果
（訊息、回覆）之前；只有同一步產生的 `Apply` 排在 barrier 前面，因為它們只涉及這個 barrier 之前就已 commit 的 entry
（S4 起，[教學 09](lessons/09-apply-overtaken-by-its-barrier/README.md)）。單一 executor 先 write + sync_all 成功，再呼叫
`Core::persisted(barrier_id) -> Vec<Effect>`，並在執行原清單後續效果之前先執行 completion 的效果（兩個 host 都如此）；
Apply 因此維持 index 順序。completion 所產生的 Apply 仍須先通過 current-term durable quorum 條件。

persisted 必須核對 pending ID；只有完成的狀態可增加 durable_index 與 leader
self-match。Core 在 pending barrier 期間不接受下一個 consensus event。executor
不得把尚未完成的內部變更當作 durable 證據。同步失敗呼叫 storage_failed，停止
整份剩餘清單並 fail closed；不得回 ACK、成功 client reply 或繼續事件處理。

`Effect::Apply(LogEntry)` 僅代表有序 committed entry；client 成功由 runtime 在
本地 KV apply 完成後回覆。新 GET 與 duplicate retry 都經安全的共識路徑。

## 計時、網路與模擬

runtime/simulator 收到 ResetElection 後，各自以 injected randomness 選 400～800ms，並另排一個固定 400ms 的
LeaderContactExpired（同一個 generation，[成員變更規格](MEMBERSHIP.md) §9.4）；
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
