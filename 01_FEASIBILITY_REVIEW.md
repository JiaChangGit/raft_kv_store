# 01. 可行性與架構審查（Architecture Review）

**審查結果：CONDITIONAL GO**。範圍可執行，但下列 P0 風險必須在對應 Gate 前被實作及測試證據消除。這是**設計審查**，不是對尚未生成程式的測試通過聲明。

## 1. 實現可行性

| 維度 | 結論 | 原因 / 約束 |
|---|---|---|
| Rust stable | GO | 能降低記憶體安全與 Thread ownership 風險，無法取代 Raft logical correctness |
| Windows 11 + WSL2 Ubuntu | GO | 在 Linux home/ext4 中執行三個 Process 與 WAL，不在 `/mnt/c` 量測 Durability |
| 12 GB RAM | GO | 三個小型 Rust process、Python proxy、pytest、有限 WAL；無 snapshot，因此測試資料需控制 |
| 3 Nodes、1 Raft Group | GO | 3 節點 Quorum=2；支援 1 node 故障，不能同時承受 2 nodes 故障而繼續提交 |
| TCP + multithreading | GO with constraints | 必須使用 single-owner event loop、framed messages、single in-flight AppendEntries/peer |
| WAL persistence | GO with constraints | 同步 WAL transaction + CRC + fsync；主機斷電/壞碟不在完整保證範圍 |
| Linearizable read | GO with design change | 先採「GET 寫入 Raft Log」，避免 V1 自製不正確的 lease/read index |
| Random fault simulation | GO | 虛擬時間 + 可重現 seed + **正式 raft-core 重用**；不可另造簡版演算法 |
| Codex phased execution | GO with human gates | Codex 可以按 spec 改檔並跑測試；必須逐階段審核，不能以自行宣稱代替驗收 |

## 2. P0 Blocking Findings — 必修正

### AR-01：Persistence ordering / RPC response barrier — P0

風險：Raft 邏輯先更新 RAM、立刻回 RPC Success，WAL 在稍後才落盤。若 Crash，Leader 會錯誤計入一個不存在的 Durable ACK。

**決議**：`step(Event)` 可規劃有序 effects，但 runtime 必須在 `Persist` 同步成功後才執行相依的 Send/Reply/Apply/Local-Durable-Match effects。V1 用單一 actor + 同步 fsync 減少 Pending I/O 狀態；失敗後停止不安全的操作，Fail Closed。

**Gate**：P03，`append_ack_after_durable_sync` 與真實 crash failpoints。

### AR-02：Current-term commit restriction — P0

風險：看到舊 Term Entry 在過半數節點上就直接將其視為 committed。

**決議**：只有 `N > commitIndex`、多數 `matchIndex >= N`、且 `log[N].term == currentTerm` 才能將 Leader `commitIndex` 提升至 N。之前的 prefix 因當前 Term Entry Commit 而間接 Commit。

**Gate**：P02，`previous_term_entry_requires_current_term_commit`，並以 paper Figure 8 推導說明。

### AR-03：Recovery cannot apply all WAL entries — P0

風險：WAL 裡的 Log 可能還沒 Commit，Restart 後直接 replay 為 KV 會造成外部可見資料錯誤。

**決議**：重啟時回復 persistent term/vote/log；`role=Follower`、`commitIndex=lastApplied=0`，KV/Session 從已證實 committed 的 prefix 依序 apply；新 Leader 先 Append 並 Commit current-term NoOp。Follower 由安全的 `leaderCommit` 通知推進，不能 apply 不匹配的 suffix。

**Gate**：P03，`uncommitted_suffix_not_applied_after_recovery`。

### AR-04：Linearizable read on isolated old leader — P0

風險：舊 Leader 在 Partition 後仍有舊 KV Map，直接 local GET 造成 Stale Read。

**決議**：V1 所有新 GET 均作為 Replicated Log Entry，在 Commit + Apply 後回結果；ReadIndex/Lease 不納入 V1。對重試/快取也不可在缺乏安全前提時直接由舊 Leader 回成功。

**Gate**：P05、P07，`replicated_get_linearizable` 與 partition tests。

### AR-05：Client retry exactly-once effects — P0

風險：Client Timeout 發生在 Entry Commit 後但回覆前，新 Leader 收到重試可能造成重複應用。

**決議**：透過 replicated Session Table 保存 `(client_id, seq, canonical_command, result)`，由 committed log 重建；單一 Client 只允許一個 in-flight logical request，重試必須保持同一 Sequence。相同 ID 不同內容明確拒絕，不能只依賴 Leader RAM Cache。

**Gate**：P05、P07，`retry_same_request_has_single_effect`、`same_id_different_command_rejected`、`test_retry_after_leader_crash`。

### AR-06：Simulation must match production core — P0

風險：寫兩套 Raft（Simulation 與 Production），模擬通過也無法說明生產邏輯正確。

**決議**：同一 `raft-core` 的 `step(Event)` / effect contract 注入 Virtual Clock / Fake Disk / Fake Network 或真實 runtime；所有不變條件由獨立 checker 監測。

**Gate**：P00、P07，`simulation_seed_is_replayable`，random history run。

## 3. P1 Non-blocking, but must have acceptance coverage

| ID | 風險 | 對策 |
|---|---|---|
| AR-07 | AppendEntries 回應延遲造成 `matchIndex` 假性進展 | 每 Peer 限制 1 in-flight；rpc_id + term/epoch 驗證；回覆只確認本次匹配 prefix |
| AR-08 | Follower 收到 heartbeat 後用自己的 last index apply 衝突 suffix | Commit 範圍最多到本次被確認匹配的 prefix end |
| AR-09 | 某 RPC 在升 Term 時 reset timer 過度，造成 liveness failure | 只在接收有效 leader AE / grant vote 等指定事件重置 election timeout |
| AR-10 | 錯誤的 WAL 尾部修復破壞 log safety | 只接受可證明末尾不完整 record；中間 CRC/error Fail Closed |
| AR-11 | Python Fault Proxy 誤對 TCP byte stream reorder | 僅對解析完成的 RPC frame 故障注入；測試不得繞過 Proxy |
| AR-12 | Gate `cargo test` 有 0 tests 仍 exit 0 | 先 Test Discovery 實際檢查 gate 名稱，再執行完整 workspace tests |
| AR-13 | WSL `/mnt/c` fsync 與 Linux 原生行為差異 | WAL / benchmark 位於 WSL2 ext4，公開平台限制 |
| AR-14 | No snapshot 下 WAL 永遠增長 | 明確測試資料限制、磁碟空間保護、排除長期資料庫運維用途 |

## 4. 假設與非保證

- 非 Byzantine：Process 可 stop/crash/restart，網路可 delay/drop/duplicate/reorder frames/partition；節點 ID / config 不被惡意偽造。
- 沒有 Dynamic Membership，固定 3 個資料目錄；**不是** 2 節點也能保證讀寫的 Active-Active Database。
- 安全性不依賴固定 clock speed；Liveness 需要最終穩定網路、多數節點正常、fsync 可完成及足夠時間。
- 認定 OS/SSD 正確實現成功返回的 `sync_all` durability；不承諾磁碟毀損、任意 power-loss、惡意資料篡改。
- V1 無 snapshot/compaction，測試規模應有限且執行前檢查可用磁碟空間。
- JSON RPC 為便於 debug 的選擇；不是高性能 production wire spec。

## 5. 刻意修正先前工程提案的模糊處

1. **Follower 取得 heartbeat 並不等於整份 Log 都被 Leader 驗證**；只能使用 `prevLogIndex` 加本次 AppendEntries 的連續 entries 作為已匹配範圍。
2. **Un-fsynced WAL record 可能在 Crash 後意外完整存在**，不能假設未 fsync 必然消失；保證的是「已 ACK 的必須 durable」，不是「未 ACK 的必定丟失」。
3. **單 Thread actor 不等於自動達成 durability**；effect ordering 必須具體有阻擋相依 Response 的 barrier。
4. **Client duplicate cache 不能取代 Quorum 確認**；尤其不得把過時 Leader 的 cache 回覆當作新的 GET 成功。
5. **單次測試成功不是 Safety Proof**。Formal proof 不在 V1，但精確反例與 invariant checker 是必交成果。

## 6. 審查決策

**CONDITIONAL GO → Engineering GO 的條件：** P00 Contracts 定義清楚；P01 Election 有負向測試；P02 current-term commit/collision tests；P03 durability/failpoint tests；P05 Linearizable GET 與重試；P06/P07 真實 process、partition 與 checker 過關。

**禁止偷跑：** 前一 Gate 未通過，不可跳到下一階段或以「差不多能跑」作為完成標準。
