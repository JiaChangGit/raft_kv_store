# S3 規格：ReadIndex 讀取

狀態：**已核准（2026-11-13），實作中。** 本文件沒有任何一項已被測試驗證；結果以驗收階段 12 為準。
已確認的決定見 §12。

用語：**ReadIndex** 是讓 GET 不寫進 log、仍保持線性一致的讀取方法。**readIndex** 是 leader 為某一批讀取記下的 commit index。
**確認回合**（confirmation round）是 leader 為了確認自己仍是 leader 而送出的一輪 AppendEntries；只有「讀取到達之後才送出」的那一輪才算數。

## 1. 為什麼要做

目前每一個 GET 都和寫入一樣走完整的複製流程（讀程式得知）：

| 步驟 | 代價 |
|---|---|
| leader 把 GET 附加到 log，寫 WAL 並 `fsync` | 每個 GET 一次磁碟同步 |
| 複製給 follower，多數各自寫 WAL 並 `fsync` | 再加至少一次 |
| commit 後套用，在 session 中記住這筆 GET 的結果 | 記憶體；S2 起也進入快照 |
| WAL 因讀取而成長 | 觸發更多次壓縮 |

讀取不改變任何狀態，卻付出和寫入一樣的磁碟代價。論文 §8 指出讀取可以不經過 log，只要多兩個防護；博士論文 §6.4 把它寫成五個步驟。

## 2. 來源

摘錄見 `docs/reference/raft-notes.md`「唯讀請求」一節，引用前回原文核對。

- 論文 §8：不經 log 的讀取有讀到舊資料的風險，因為回覆的 leader 可能已被不知情地取代。兩項防護：leader 必須先 commit 一筆自己任期的 entry
  （"each leader commit a blank no-op entry into the log at the start of its term"）；回覆前與多數交換 heartbeat，確認自己沒有被取代。
- 博士論文 §6.4 的五步：(1) 尚未 commit 本任期的 entry 就等待；(2) 把當下的 commit index 記為 readIndex；(3) 送出新一輪 heartbeat，等待多數回覆；
  (4) 等狀態機至少套用到 readIndex；(5) 執行查詢並回覆。並指出一輪 heartbeat 可以服務累積的任意多筆讀取。
- 博士論文 §6.4 的 follower 讀取與 §6.4.1 的 lease：本階段不做（§12 D2；`AGENTS.md` 永久禁止 lease）。

## 3. 範圍

**要做**

1. core 的讀取流程：§6 的五步，含批次確認。
2. 節點把新的 GET 一律改走 ReadIndex，不再寫進 log（§12 D1）。
3. GET 重送時重新讀取；GET 不再寫進 session（§12 D3）。用戶端契約與線性一致性檢查器同步修改。
4. 模擬器以讀取事件取代「把 GET 寫進 log」，並新增讀取新鮮度不變量 I8。
5. 真實三程序測試、mutation、驗收階段 12、前後實測（§12 D4）、教學。

**不做**

- follower 讀取、lease、依時鐘的任何最佳化。
- 為讀取另開一條不受「每個 follower 一筆進行中請求」限制的 RPC。確認回合沿用 AppendEntries（§6.2），最壞多等一個來回。
- check-quorum（leader 收不到多數回覆時主動下台）。目前被孤立的 leader 會一直以為自己是 leader；它的讀取不會被回覆，只會逾時（§6.4）。

## 4. 保證與不保證

**保證（都要有對應測試，見 §10）**

- G1 **線性一致**：讀取回覆的狀態至少包含「這筆讀取開始之前，叢集中任何節點已知 commit 的所有 entry」。這比線性一致的最低要求更嚴格，是博士論文 §6.4 第 3 步的論證所給出的性質，模擬器以此檢查（I8）。
- G2 **讀取不寫磁碟**：GET 不會在任何節點產生 WAL record，也不進入 session 或快照。
- G3 **無法連絡多數的 leader 不回覆讀取**：只會失敗或逾時，不會回覆舊資料。
- G4 **新 leader 先 commit 本任期的 entry 才回覆讀取**。
- G5 **批次**：一個確認回合完成時，所有等待它的讀取一起回覆。
- G6 **失去 leader 身分時，等待中的讀取明確失敗**（`NOT_LEADER`）。讀取沒有副作用，所以這是確定的拒絕，不是「結果未知」。

**不保證**

- N1 讀取延遲：最壞約兩個來回（§6.2），沒有上限保證。
- N2 讀取量受單一 leader 限制（沒有 follower 讀取）。
- N3 同一筆 GET 重送時，結果可能比第一次更新。這仍是線性一致的（它落在這個操作的開始與結束之間），但和 V1 的「回覆原結果」不同。
- N4 被孤立的 leader 上等待中的讀取，要到逾時或它得知更高的 term 才結束。

## 5. 對外行為的改變

| 項目 | S2 | S3 |
|---|---|---|
| 新的 GET | 寫進 log，commit 並套用後回覆 | 走 ReadIndex，不寫 log |
| GET 重送（相同 client_id 與 sequence） | 回覆第一次的結果與 committed_index | 重新讀取 |
| GET 與 session | 記入 session（每個 client 最新一筆） | 不記入；不影響 session |
| `sequence` 小於該 client 最新寫入的 GET | `INVALID_ARGUMENT` | 照常讀取（讀取不經過 session） |
| GET 與寫入共用同一個 `(client_id, sequence)` | 後到者 `REQUEST_ID_REUSE` | GET 照常讀取，不檢查 |
| `effect_count` | 含 GET | 只計寫入（Put、Delete） |
| 回覆的 `committed_index` | GET 那筆 entry 的 index | 讀取當下狀態機已套用到的 index（不小於 readIndex） |
| follower 收到 GET | `NOT_LEADER` | 不變 |
| leader 失去身分時等待中的 GET | `TIMEOUT_UNKNOWN` | `NOT_LEADER` |

狀態機仍然能套用 log 中既有的 Get entry（例如舊的 WAL 或快照之前的資料），行為與 S2 相同；只是節點不再產生新的 Get entry。
用戶端仍應為每個新請求遞增 `sequence`（包括 GET），契約不變，只是 GET 不再依此去重。

## 6. 演算法

### 6.1 core 的狀態

| 欄位 | 意義 |
|---|---|
| `read_round` | leader 目前的確認回合編號，每開始一個新回合就加一 |
| `acked_round[p]` | follower p 已回覆過的最大回合編號 |
| 進行中請求的 `round` | 這筆 AppendEntries（或 InstallSnapshot）送出時的 `read_round` |
| `pending_reads` | 等待中的讀取：讀取編號、readIndex（尚未取得時為空）、所屬回合 |

### 6.2 流程

新事件 `Event::Read { id }`（`id` 由節點程式指定）、`Event::CancelRead { id }`；新效果 `Effect::ReadReady { id, read_index }`、`Effect::ReadFailed { id, status }`。

1. **不是 leader** → 立即 `ReadFailed(NOT_LEADER)`。
2. **本任期尚未有 entry commit**（`term_at(commit_index) != current_term`）→ 放進 `pending_reads`，readIndex 留空，等待。
   leader 當選時會附加一筆 NoOp（V1 已有），它 commit 後條件就成立。
3. **條件成立時**（讀取到達時已成立，或之後因 commit 前進而成立），對所有 readIndex 為空的讀取：readIndex = 當下的 `commit_index`；
   `read_round += 1`，這些讀取屬於新的回合；對每個 follower 呼叫 `replicate`。
4. **送出請求時**把當下的 `read_round` 記在進行中請求上。因為回合編號在讀取登記時才加一，**讀取到達之前送出的請求，回合編號一定比較小**。
   若某個 follower 已有進行中的請求（回合較舊），就等它回覆後再送一筆新的；所以最壞多等一個來回。
5. **收到回覆**：只要回覆的 term 等於目前的 term、且對應到進行中的那筆請求（不論 `success` 是真是假），就把 `acked_round[p]` 更新為那筆請求的回合。
   term 相同代表那個 follower 在回覆時沒有見過更高的 term，仍承認這個 leader。`InstallSnapshotResponse` 同樣計入。
6. **確認**：某個回合 r，若「自己」加上 `acked_round[p] >= r` 的 follower 達到多數，屬於 r 以前（含）的讀取都已確認。
7. **回覆**：已確認且 `last_applied >= readIndex` 的讀取發出 `ReadReady`。leader 的套用在 commit 前進時同步完成，通常立即成立；這個條件仍明確檢查。
8. **失去 leader 身分**（step down、term 改變）→ 所有等待中的讀取 `ReadFailed(NOT_LEADER)`，回合資料清空。
9. `CancelRead { id }` → 從等待中移除（節點程式在用戶端逾時後呼叫，避免等待清單無限成長）。

讀取不會改變持久化狀態，因此不會產生 `Persist` 或 `PersistSnapshot`。

### 6.3 為什麼這樣是安全的

（依博士論文 §6.4 第 1、3 步的論證，套用到本專案的實作；屬於論證，不是形式化證明。）

- 讀取開始之後，leader 送出了回合 r 的請求，多數以相同的 term 回覆。要在更高的 term 選出新 leader，必須有多數投票，而投票者會把 term 提高；
  兩個多數必然重疊，所以在那些回覆送出時，還沒有任何更高 term 的 leader 能完成 commit。因此在讀取開始時，**這個 leader 的 commit index 不小於叢集中任何節點已知的 commit index**。
- 第 2 步保證這個 commit index 已經涵蓋前任 leader commit 的所有 entry（Leader Completeness 加上本任期 entry 已 commit）。
- 兩者合起來就是 G1。第 4 步的「回合在讀取到達時才加一」是整個論證的關鍵：若用了讀取到達之前送出的請求的回覆，第一點就不成立。

### 6.4 節點程式

- 收到 GET：分配讀取編號，記下用戶端的回覆通道與到期時間，送出 `Event::Read`。
- `ReadReady`：從 KV 讀取，回覆 `OK`、結果、`committed_index` 為狀態機目前套用到的 index。
- `ReadFailed`：回覆對應的狀態與 leader hint。
- 到期的讀取：從清單移除並送出 `CancelRead`；用戶端那邊已經以 `TIMEOUT_UNKNOWN` 結束。
- 等待中的讀取計入現有的「等待中請求上限」（1,024 筆）。
- 狀態查詢新增 `reads_served`、`reads_failed`。

## 7. 和其他功能的關係

- **S2 快照**：readIndex 不小於 commit index，而 commit index 不小於快照位置，所以讀取總是落在已套用的範圍內。InstallSnapshot 的回覆也能確認回合。
- **S4 成員變更**：「多數」目前是固定的 2/3。實作時集中在一個計算多數的函式，S4 改成依目前的成員設定計算，ReadIndex 與 commit 用同一份設定。
- **S5 效能**：GET 不再寫磁碟後，寫入與讀取的比例會影響 S5 的量測設計。

## 8. 模擬器

- 用戶端工作負載中的 GET 改為對 leader 送出 `Event::Read`，不再 `Propose`。
- 新增 I8 **讀取新鮮度**：讀取開始時，記下觀察者已知的最大已 commit index；`ReadReady` 的 readIndex 必須不小於它，且該節點的模型狀態機至少套用到 readIndex。
- 原有的訊息遺失、延遲、重複、crash、分區都照舊作用於確認回合。

## 9. 量測（§12 D4）

`scripts/read_benchmark.py`：在同一台機器上，分別用 tag `s2-snapshot-compaction-verified` 的執行檔（在暫存目錄重新建置）與目前的執行檔啟動三節點叢集，
各做一組連續 GET，記錄延遲的 p50、p95、p99，以及每個 GET 讓各節點 WAL 成長多少位元組。結果寫入 `artifacts/reports/read-benchmark.json`，附上來源指紋與機器資訊。
**數字只記錄，不設門檻**：它們隨機器與負載變動。確定性的部分由測試保證（G2：讀取不讓 WAL 成長）。

## 10. 測試與驗收（階段 12）

**core 單元測試**：讀取要等本任期 entry commit；讀取到達前送出的請求的回覆不算確認；一個回合確認多筆讀取；確認後仍要等套用到 readIndex；
失去 leader 身分時等待中的讀取全部失敗；follower 拒絕讀取；讀取不產生持久化效果；取消的讀取不再回覆；`success = false` 但 term 相同的回覆也算確認。

**檢查器**：GET 不使用 session；同一筆 GET 重送可得到較新的值；舊 sequence 的 GET 照常讀取；既有的「讀到舊值必須判為失敗」測試照舊。

**真實三程序**：
- 連續 100 個 GET 前後，每個節點的 `durable_sequence` 與 WAL 大小都不變，且讀到的值正確。
- 寫入後殺掉 leader，新 leader 一選出就讀取，必須讀到那筆寫入（G4）。
- 既有的 `test_partitioned_leader_cannot_commit_or_read`：被孤立的 leader 不能回覆讀取（G3）。
- 既有的隨機故障線性一致測試在 ReadIndex 下照常通過。

**模擬器**：1,000 個種子 × 10,000 個事件，I1～I8 全部成立。

**Mutation**（至少）：不確認就回覆；接受讀取到達前送出的請求的回覆；不等本任期 entry commit；失去身分時不讓等待中的讀取失敗；不等套用到 readIndex 就回覆。

**對既有測試的影響**（實作時記錄於契約）：
- S2 的 `test_full_restart_recovers_from_snapshots_and_sessions` 斷言「新的 GET 讓 `effect_count` 加一」；S3 起 GET 不計入，改為斷言不變，並另外確認 WAL 未成長。
- 檢查器的 `test_unseen_lower_sequence_is_rejected` 以 GET 示範舊 sequence 被拒；改以寫入示範，另加 GET 照常讀取的案例。
- v1 契約的 `replicated_get_linearizable` 驗證 core 能把 GET 當成一般命令複製；core 仍保有這個能力（狀態機也仍能套用），但節點不再這樣用。對照表補記。

**Bug 教學**：實作中遇到的每個 bug 寫成教學。

## 11. 實作順序

1. core 的讀取流程與單元測試（節點尚未使用，現有測試照過）。
2. 節點改用讀取流程、檢查器與用戶端契約同步修改、S2 測試調整——這三者必須一起完成，否則整合測試會在新舊語意之間不一致。
3. 模擬器的讀取與 I8。
4. 三程序測試、量測腳本、mutation、驗收階段 12、教學、文件。

每一步結束時全部測試通過；並先用 `scripts/preflight_phase.py` 檢查新階段，再跑完整驗收。

## 12. 已確認的決定（2026-11-13）

| 編號 | 問題 | 決定 |
|---|---|---|
| D1 | 舊的「GET 寫進 log」路徑 | 新的 GET 全部改走 ReadIndex；效能對照用 tag s2 的舊執行檔，不保留切換參數 |
| D2 | follower 讀取 | 只由 leader 服務 |
| D3 | GET 重送 | 重新讀取；GET 不再寫進 session，`effect_count` 不計 GET |
| D4 | 效能實測 | 要，只記錄不設門檻；另以測試保證讀取不讓 WAL 成長 |

核准本規格後，`AGENTS.md` 在同一個 commit 修改：解除「不做 ReadIndex」；把「新的 GET 一律複製」改為本規格的讀取規則；
去重規則註明只適用於寫入。lease 的禁止維持不變。
