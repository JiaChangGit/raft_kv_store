# S2 規格：快照與 log 壓縮

狀態：**已核准（2026-11-13）並實作**，驗收結果以 `scripts/status.py` 與 `artifacts/acceptance-final/` 的階段 11 為準。
下文保留核准時的原文；實作中發現的偏離與補記集中在 §14。

用語：**快照**（snapshot）是狀態機在某個 log 位置的完整內容。**壓縮**（compaction）是做完快照後丟掉被它涵蓋的 log。
**base** 指目前快照涵蓋到的最後一個 index 與它的 term；base 之前的 log 已不存在。**落後的節點**指它需要的 log 已被 leader 丟掉的 follower。

## 1. 為什麼要做

目前三件事會隨使用時間無限成長：

| 項目 | 現況（讀程式得知） | 後果 |
|---|---|---|
| WAL | 每筆交易都附加在 `raft.wal`，從不刪除 | 磁碟用量無上限；重啟要讀完整個檔案 |
| 記憶體中的 log | `DurableState.log` 保留第 1 筆起的全部 entry | 記憶體無上限；core 每個事件都複製並比對整份狀態，成本隨 log 長度成長 |
| 去重 session | `kv-state` 以 `(client_id, sequence)` 為鍵，**每一筆請求**都永久保存完整的操作與回覆（一筆 Put 最多約 8 KiB） | 記憶體隨請求次數無上限 |

另外，節點重啟後要從第 1 筆重新套用到狀態機，log 越長越慢。

## 2. 來源

本節以實際讀過的原文為準，摘錄見 `docs/reference/raft-notes.md`。

- 論文 §7 與 Figure 12、13：快照內容（last included index／term、該位置的成員設定）、InstallSnapshot RPC 的欄位與接收步驟、
  follower 收到快照時保留或丟棄既有 log 的規則。
- 博士論文 §5.1："State machines must also serialize the information they keep for providing linearizability to clients"——
  去重 session 必須放進快照。快照完成後 "Raft first stores the state it needs for a restart … Then it discards the prefix of its log"。
- 博士論文 §5.1.2：以「log 大小超過上次快照大小 × 倍數」決定何時做快照，倍數 4 時約 20% 的磁碟頻寬用於快照。
- 博士論文 §5.1.3：程式原本預設「有第 i 筆就有 1～i-1 筆」，壓縮後不成立，作者希望由型別系統強制處理越界；
  並建議開發期間「每套用一筆就做一次快照」以盡早抓到 bug；先寫入暫存檔、flush 後再 rename。
- 博士論文 §6.3：session 的淘汰必須是確定性的。

## 3. 範圍

**要做**

1. log 改為「base＋其後的 entry」結構，所有存取都經過會回報「已被壓縮」的介面。
2. 狀態機（KV＋session）可序列化成確定性的位元組，並可從中還原。
3. session 改為每個 client 只保留最新一筆（§13 D1）。
4. 快照檔格式、WAL 檔頭的 base 欄位正式啟用、兩者在 crash 後的一致性規則。
5. 本機壓縮：依倍數規則（測試時可改為每 N 筆）做快照並丟棄 log 前段。
6. InstallSnapshot RPC：分塊傳送給落後的節點（§13 D2）。
7. 模擬器、真實三程序測試、mutation 與驗收階段 11。

**不做（本階段刻意排除）**

- 與正常處理同時進行的快照（論文 §5.1.1 的 copy-on-write／fork）。快照在 actor 內同步完成，期間節點暫停處理；耗時會記錄。
- 壓縮、增量快照、磁碟型狀態機、follower 讀取、session 依時間淘汰、client 註冊。
- 把 core 的「每個事件複製並比對整份狀態」改成追蹤變動：延到 S5，以 benchmark 決定（§13 D4）。壓縮會把 log 長度限制在門檻附近，複製成本因此有上限。

## 4. 保證與不保證

**保證（每項都要有對應測試，見 §11）**

- G1 **只丟已套用的資料**：本機壓縮只涵蓋已套用（必然已 commit）的 entry。
- G2 **先落盤、再丟棄、再回覆**：快照檔落盤之後才改寫 WAL 丟掉前段；follower 安裝快照，要在快照與 WAL 都落盤後才回覆成功。
- G3 **任何中斷都能回復**：壓縮或安裝在任一步被終止，重啟後得到舊狀態或新狀態之一；超出 §7.3 允許範圍的不一致一律 fail closed。
- G4 **快照等價於重放**：在 index S 還原快照，得到的 KV、session 與 `effect_count` 和從頭套用 1～S 完全相同；三台節點在同一個 S 做出的快照位元組完全相同。
- G5 **有界**：WAL 大小不超過「1 MiB 與上次快照 4 倍兩者取大」再加上尚未套用的 entry；快照不超過 64 MiB；session 數等於 client 數。
- G6 **落後的節點追得上**：leader 已丟掉某 follower 需要的 entry 時，改送快照，follower 追上後回到正常複製。
- G7 **既有安全性不變**：模擬器在開啟壓縮（含每筆都壓縮）的情況下，I1～I5 照樣成立；新增 I6、I7（§10）。

**不保證**

- N1 KV 資料量本身沒有上限。序列化後超過 64 MiB 時，該節點停止壓縮並記錄，log 從此恢復成長；不會截斷資料。
- N2 client 數沒有上限，session 也就沒有上限（§13 D1 的取捨）。
- N3 做快照期間節點不處理其他事件；資料很大時可能拖過選舉逾時，引發一次換 leader。這是可用性問題，不影響安全性；耗時會量測並寫進結果。
- N4 快照的 CRC 只防意外毀損。一個有 bug 的 leader 若送出 CRC 正確但內容錯誤的快照，follower 會在解析時發現並 fail closed（停止服務），不會嘗試修復。
- N5 主機斷電沿用 S1 的限制：只測 process 被 SIGKILL。

## 5. log 的新結構

`DurableState` 改為：

```rust
pub struct LogBase { pub index: LogIndex, pub term: Term }   // (0, 0) 表示沒有快照
pub struct DurableState {
    pub term: Term,
    pub voted_for: Option<NodeId>,
    base: LogBase,          // 私有
    entries: Vec<LogEntry>, // 私有：index 為 base.index+1 起連續
}
```

`entries` 不公開，外部只能透過下列方法存取。這是採用博士論文 §5.1.3 的建議：讓編譯器找出每一個原本假設「log 從 1 開始」的地方。

| 方法 | 回傳 |
|---|---|
| `base()` | 目前的 base |
| `last_index()` | `base.index + entries.len()` |
| `term_at(i)` | `i == base.index` 時是 `base.term`；`i < base.index` 時是 `None`（已壓縮）；超過 `last_index` 也是 `None` |
| `entry(i)` | `i` 在 `base.index+1 ..= last_index` 才有 |
| `entries_from(i)` | 從 `i` 起的切片；`i <= base.index` 時回報錯誤 |
| `truncate_from(i)`、`push(entry)` | 只能動 base 之後的部分 |
| `with_base(base, entries)` | 建構；檢查 index 連續、term 不遞減、`base.term <= term` |

另新增一個純函式 `adopt_snapshot(state, base) -> DurableState`，由 core（安裝快照）、儲存層（重啟時）與模擬器共用，規則照 Figure 13 第 6、7 步：

- 若 `state` 在 `base.index` 有 entry 且 term 等於 `base.term`：保留其後的 entry。
- 否則（沒有這筆、或 term 不同）：丟掉全部 entry。
- term、投票不變。

## 6. 狀態機快照

### 6.1 session 改為每個 client 一筆

```text
sessions: client_id -> { sequence, operation, response }
```

套用一筆 `(client_id, sequence, operation)`：

| 情況 | 結果 | 與現況比較 |
|---|---|---|
| 沒有這個 client 的紀錄 | 執行，存成它的紀錄 | 相同 |
| `sequence` 等於紀錄的 | operation 相同 → 回覆原結果；不同 → `REQUEST_ID_REUSE`；都不執行 | 相同 |
| `sequence` 大於紀錄的 | 執行，取代紀錄 | 相同 |
| `sequence` 小於紀錄的 | `INVALID_ARGUMENT`，不執行 | **改變**：原本若那筆仍在記錄中會回覆原結果 |

最後一列只有在 client 違反「一次只有一筆未完成請求」的契約、或同一請求的延遲副本在更新的請求之後才被 commit 時才會出現。
兩種情況都不會重複執行，差別只在回覆內容；而後者發生時，client 早已收到原結果並送出了新請求，沒有人在等這個回覆。
需同步修改：`AGENTS.md` 的去重條文、`docs/CLIENT_CONTRACT.md`、`tools/linearizability.py` 的模型與其測試。
（實作時補記：v1 契約的 `test_retry_after_leader_crash` 原本以「舊 sequence＋不同指令」驗證 `REQUEST_ID_REUSE`，
本規格漏列。已改為分別驗證「最新 sequence＋不同指令 → `REQUEST_ID_REUSE`」與「舊 sequence → `INVALID_ARGUMENT`」，
兩者都另外確認 KV 未改變；對照記於契約的 `v1_mapping`。）
原本的 `highest_sequence` 併入這份紀錄。

### 6.2 序列化

快照內容是 UTF-8 JSON，欄位依序：`applied_index`、`effect_count`、`values`（key 排序）、`sessions`（以十進位字串表示的 client_id 排序）。
全部使用排序過的 map，因此相同狀態必然產生相同位元組（G4 的測試以此比對三台節點）。還原時檢查 `applied_index` 等於快照的 base index，
且每個 key、value 符合大小限制；不符即 fail closed。

## 7. 磁碟上的格式與一致性

### 7.1 快照檔 `snapshot`

| offset | bytes | 內容 |
|---:|---:|---|
| 0 | 4 | ASCII `RFTS` |
| 4 | 2 | 版本 1 |
| 6 | 2 | 檔頭長度 64 |
| 8 | 24 | `cluster_id`，尾端補 0 |
| 32 | 8 | `last_included_index`（> 0） |
| 40 | 8 | `last_included_term`（> 0） |
| 48 | 4 | 成員：bit n 代表節點 n；本階段必須是 `0b1110`（節點 1、2、3） |
| 52 | 4 | `payload_len`，1 ～ 64 MiB |
| 56 | 4 | payload 的 CRC32C |
| 60 | 4 | 檔頭 CRC32C，涵蓋 0～59 |
| 64 | `payload_len` | §6.2 的 JSON |

成員欄位是為 S4 預留（論文要求快照記錄該位置的成員設定）；本階段只接受固定的三台。
目錄中同時只有一份 `snapshot`；新的以 rename 取代舊的。讀取時任何欄位或 CRC 不符都 fail closed，不改任何檔案。

### 7.2 WAL 檔頭欄位啟用

S1 預留的五個欄位開始使用：`base_index`、`base_term`、`start_term`、`start_voted_for`、`first_sequence`。
WAL 的初始狀態改由檔頭決定：`term = start_term`、`voted_for = start_voted_for`、log 的 base 為 `(base_index, base_term)`、
第一筆 record 的 sequence 必須是 `first_sequence`。檔頭檢查改為驗證這些欄位的關係（`base_index` 為 0 時 `base_term` 也必須為 0、
`base_term <= start_term`、投票為 0～3、`first_sequence >= 1`），不再要求固定值。transaction 的 `truncate_from` 必須大於 `base_index`。

### 7.3 重啟時快照與 WAL 的一致性

設快照的 index 為 S（沒有快照則視為 0），WAL 檔頭的 `base_index` 為 B。

| 情況 | 處理 | 何時會出現 |
|---|---|---|
| 無快照、B = 0 | 照常 | 從未壓縮 |
| 無快照、B > 0 | **fail closed** | 快照檔遺失 |
| S > 0、B = S | WAL 的 `base_term` 必須等於快照 term，否則 fail closed | 正常壓縮後 |
| S > 0、B > S | **fail closed** | 不可能由本程式的寫入順序產生 |
| S > 0、B < S | 用 `adopt_snapshot` 得到新狀態，**立刻改寫 WAL**（暫存檔、`sync_all`、rename、同步目錄），再繼續 | 壓縮或安裝在「快照已落盤、WAL 還沒改寫」之間被中斷 |

最後一列是「往前完成」：快照已經落盤，代表它涵蓋的 entry 都已 commit，所以依快照完成壓縮是安全的。
改寫完成前再被中斷，下次重啟會走同一列，結果相同。殘留的 `snapshot.tmp`、`raft.wal.tmp` 一律忽略並刪除。

重啟後：狀態機由快照內容還原（沒有快照則為空）；core 的 `commit_index` 與 `last_applied` 設為 S，而不是 0。
這是 S2 對「重啟只還原 term、vote、log」這條規則的修改：快照只含已 commit 的 entry，所以直接當作已套用是安全的；
S 之後的 entry 仍和以前一樣，要重新得知已 commit 才套用。

### 7.4 寫入順序（本機壓縮與安裝快照共用）

1. 寫 `snapshot.tmp`，`sync_all`，rename 為 `snapshot`，同步目錄。
2. 依新狀態寫 `raft.wal.tmp`：檔頭的 base 為快照位置、`start_term`／`start_voted_for` 為目前的 term 與投票、`first_sequence` 為下一個 sequence；
   若 base 之後還有 entry，接一筆 transaction 帶上它們。`sync_all`，rename 為 `raft.wal`，同步目錄。
3. 重新開啟 WAL 檔，更新記憶體中的 durable 狀態與 sequence。

任何一步失敗都 poison，節點停止服務。每一步之間都有測試用的中斷點（§11）。

## 8. 本機壓縮

觸發（§13 D3），在 actor 每次處理完會套用 entry 的事件後檢查：

- **一般模式**：`WAL 大小 >= max(1 MiB, 4 × 上次快照大小)`，且已套用的 index 大於目前的 base。
- **測試模式**：啟動參數 `--snapshot-every N`，已套用的 index 比 base 多 N 筆以上就做。N = 1 即「每筆都做」。

流程：actor 序列化狀態機（index 為 `last_applied`）→ 超過 64 MiB 就放棄這次並記錄（N1）→ 交給 core 的新事件 `Compact { index, data }`。
core 檢查 `index == last_applied` 且 `index > base.index`，以 `term_at(index)` 為 base term，產生新狀態並發出 `PersistSnapshot` 效果；
actor 依 §7.4 寫入後呼叫 `persisted`。壓縮和一般寫入一樣走 durability barrier，等待期間 core 不處理其他事件。

## 9. InstallSnapshot RPC

### 9.1 訊息

```text
InstallSnapshot {
  term, leader_id,
  last_included_index, last_included_term, voters,
  total_len, total_crc,      // 整份快照的長度與 CRC32C；follower 用來限制記憶體並驗證
  offset,                    // 這一塊在快照中的位置
  data,                      // 這一塊的內容，小寫十六進位字串；原始資料最多 256 KiB
  done                       // 是否為最後一塊
}
InstallSnapshotResponse { term, success, last_included_index, next_offset }
```

和 Figure 13 相比多了 `voters`、`total_len`、`total_crc` 與回覆中的 `success`、`last_included_index`、`next_offset`：
前三者讓 follower 在收到第一塊時就能限制記憶體、在最後一塊時驗證整份；後三者讓 leader 知道下一塊從哪裡送、送完後 follower 到了哪裡。
`data` 用十六進位而非 JSON 數字陣列，是因為後者會讓大小膨脹約 3.5 倍、超過 1 MiB 的訊息上限；專案沒有 base64 套件，十六進位可自行實作。
分塊大小是 core 的參數：正式執行 256 KiB，模擬器用很小的值以確保多塊情境被執行到。

### 9.2 leader

- 要送給 follower p 的前一筆 `next[p] - 1` 已被壓縮（`term_at` 為 `None`）時，改送快照。
- 每個 follower 記錄傳送進度（快照身分＋offset），依回覆的 `next_offset` 送下一塊；逾時則重送同一塊。
- leader 在傳送期間又做了新快照：下一塊改從新快照的 offset 0 開始。
- 收到 `success` 且 `last_included_index` 為 S：`match[p] = max(match[p], S)`、`next[p] = S + 1`，回到 AppendEntries。
- InstallSnapshot 和 AppendEntries 一樣算作 heartbeat，follower 收到後重設選舉計時並記下 leader。

### 9.3 follower

1. `term < currentTerm`：回覆失敗並附上自己的 term（Figure 13 第 1 步）。term 較新則照常 step down。
2. 快照不新（`last_included_index <= commit_index`）：不安裝，回覆成功並附上 S。理由：S 以前的 entry 已在本機的 log 或快照裡且已 commit。
3. `offset == 0`：丟掉進行中的接收，開始新的；`total_len` 超過 64 MiB 就拒絕。
4. `offset` 不等於已收到的長度、或快照身分與進行中的不同：回覆失敗，`next_offset` 為已收到的長度（或 0），leader 依此重送。
5. `done` 為 false：存進記憶體，回覆成功與新的 `next_offset`。這時尚未落盤，因為還沒有回覆任何依賴它的東西。
6. `done` 為 true：檢查長度與 `total_crc`；以 `adopt_snapshot` 得到新狀態（Figure 13 第 6、7 步），發出 `PersistSnapshot`（含「要還原狀態機」的標記）。
   actor 先確認快照內容可以解析，再依 §7.4 寫入、還原 KV；core 在 `persisted` 後把 `commit_index`、`last_applied` 推到 S，**之後才**回覆成功。

### 9.4 也要改的地方

- AppendEntries 的 `prev_log_index` 小於 follower 的 base：batch 中 index 不超過 base 的 entry 視為已相符（它們已 commit，合法 leader 必然有相同內容），
  略過它們；若 batch 含有 base 那一筆，其 term 必須等於 base term，否則回覆失敗。其餘照原規則處理。沒有這條，leader 的 `next` 會遞減到 base 以下而永遠失敗。
- `Envelope::valid_for` 與 `raft-node` 收訊時的寄件者檢查要涵蓋新的訊息種類。
- 投票時比較 log 新舊用 `term_at(last_index)`，log 在 base 之後為空時就是 base term。

## 10. 模擬器

模擬器沒有 KV。改用一個極小的模型狀態機：內容是「已套用的 index」加上「所有已套用 entry 的串接 CRC」。快照就是這兩個數字。

- 每個種子隨機選一個壓縮頻率，包含「每筆都壓縮」；分塊大小設為數個 bytes，迫使快照被分成多塊。
- 模擬磁碟增加快照欄位，並模擬「快照已落盤、WAL 尚未改寫」之間的 crash，重啟時走與正式程式相同的 `adopt_snapshot` 規則。
- InstallSnapshot 和其他訊息一樣會遺失、延遲、重複。
- 既有不變量改用新的 log 介面：I3 只比較雙方都還有的 index；I4 對已被 leader 壓縮的 index 改為比對快照的串接 CRC。
- 新增 I6 **快照安全**：任何節點還原或安裝的快照 (S, CRC) 都必須等於觀察者記錄的已 commit 前綴 1～S。
- 新增 I7 **只壓縮已套用的部分**：任何節點的 base 都不超過它的 `last_applied`。
- 另檢查：follower 回覆 InstallSnapshot 成功前，快照必須已在模擬磁碟上落盤（同既有的「先落盤才回覆」檢查）。

## 11. 測試與驗收（階段 11）

測試名稱在實作時決定；以下是每一項必須證明的行為。

**單元測試**

- log 結構：每個方法在 base 前、base、base 後、超出末端的回傳；`adopt_snapshot` 的保留與丟棄兩種情況。
- 狀態機：快照後還原等於從頭重放（含 session 與重試結果）；三份獨立重放在同一 index 產生相同位元組；§6.1 表格的每一列；超過大小限制的快照被拒絕。
- 快照檔：與 S1 相同強度的毀損矩陣（逐一翻轉每個位元、固定種子的多位元組毀損），全部 fail closed 且不改檔。
- 一致性：§7.3 表格的每一列；往前完成後再次重啟結果相同。
- core：壓縮只能到 `last_applied`；leader 在需要的 entry 已被壓縮時改送快照；分塊依序、亂序、重複、中途換新快照；
  落盤前不回覆成功；不新的快照不會讓狀態倒退；prev 在 base 之前的 AppendEntries。

**真實 process**

- 壓縮與安裝的每個寫入步驟（§7.4）之間被 SIGKILL，重啟後狀態是舊的或新的之一，且再次重啟結果相同。
- 三程序叢集（`--snapshot-every` 設小值）：持續寫入時 WAL 大小維持有界、確實做過快照；
  停掉一台、寫入超過門檻後重啟它，它經由 InstallSnapshot 追上（狀態計數可觀察）、KV 一致；
  三台全部重啟後資料仍在，重送最後一筆請求仍得到原結果（session 已在快照裡）；歷史通過線性一致性檢查。

**模擬器**：沿用 1,000 個種子 × 10,000 個事件，開啟壓縮後 I1～I7 全部成立；另保留一組關閉壓縮的對照。

**Mutation**（植入後必須被指定測試抓到），至少包含：壓縮超過已套用位置、落盤前回覆安裝成功、term 不符仍保留 log 後段、
先改寫 WAL 再寫快照、略過快照 CRC、不新的快照仍被安裝、移除「prev 在 base 之前」的處理、不檢查分塊 offset。

**驗收契約**：新增階段 11 列出以上測試與 mutation 名稱。S1 的 `reserved_header_fields_must_be_pristine` 改為檢查檔頭欄位關係的新測試，
並在 `v1_mapping` 以外另記錄「被取代的 S1 測試」對照，原則同 v1：新的不得較弱。

**Bug 教學**：實作中遇到的每個 bug 依 `write-bug-lesson` 寫成教學。

## 12. 實作順序

採博士論文 §5.1.3 的提醒：在 log 真的能被丟棄之前，很多新路徑不會被系統測試執行到，所以先讓結構與存取全部安全，最後才開啟丟棄。

1. log 新結構（base 永遠是 0）：所有存取改走新介面，現有測試全部照過。
2. 狀態機序列化與 session 改制，連同用戶端契約與檢查器。
3. 快照檔、WAL 檔頭欄位、§7.3 一致性與往前完成，連同 crash 測試。
4. core 的 `Compact` 事件與 `PersistSnapshot`；actor 的觸發；模擬器的模型狀態機與壓縮。
5. InstallSnapshot（core、訊息驗證、actor、模擬器）。
6. 三程序測試、mutation、驗收階段 11、教學、文件與路線圖更新。

每一步結束時整個 workspace 的測試都要通過。

## 13. 已確認的決定（2026-11-13）

| 編號 | 問題 | 決定 |
|---|---|---|
| D1 | 去重 session 的保存方式 | 每個 client 只保留最新一筆（論文 §6.3 的做法）；client 數不設上限 |
| D2 | 快照傳送方式 | 依 Figure 13 分塊傳送；每塊 256 KiB；快照上限 64 MiB |
| D3 | 何時做快照 | 博士論文 §5.1.2 的倍數規則（倍數 4、下限 1 MiB），另提供 `--snapshot-every N` 供測試 |
| D4 | core 每個事件複製整份狀態 | 延到 S5，以 benchmark 決定；S2 靠壓縮限制 log 長度 |

核准本規格後，`AGENTS.md` 要在同一個 commit 修改三處：解除「不做 snapshot」；
重啟規則加上「可還原已落盤的快照（只含已 commit 的 entry）」；去重規則改為「每個 client 保存最新一筆的 sequence、canonical command 與原結果」。

## 14. 實作補記（2026-11-13）

| 項目 | 規格原文 | 實際情況 |
|---|---|---|
| §12 步驟 4、5 | 分成兩步，每步結束全部測試通過 | 做不到：只有本機壓縮時，第一個落後的 follower 就讓 leader 走到 `unreachable!`。兩步合併為一個 commit（`a5023d7`）。見 [教學 03](lessons/03-compaction-without-install-snapshot/README.md) |
| §6.1 需同步修改的地方 | 列了 AGENTS.md、用戶端契約、檢查器 | 漏了 v1 契約的 `test_retry_after_leader_crash`，已改寫並記錄於契約的 `v1_mapping` |
| §9.1 回覆欄位 | `term, success, last_included_index, next_offset` | 另加 `done`：follower 已持久保存到 `last_included_index`。leader 據此更新進度，模擬器據此檢查「先落盤才回覆」 |
| §11 S1 被取代的測試 | `reserved_header_fields_must_be_pristine` 改為新測試 | 新測試是 `file_header_start_fields_validated`，記錄在契約的 `replaced_tests` |
| 測試程式的偶發失敗 | 未預期 | 安裝快照的真實 process 測試與其他測試平行時偶發失敗，原因是 `flock` 被別的測試 fork 出的子程序暫時繼承。見 [教學 02](lessons/02-flaky-lock-inherited-by-child/README.md) |
| 狀態機快照的驗證 | §6.2 列了 index 與大小限制 | 寫測試時發現 `restore` 沒有限制 session 中記住的回覆值大小，已補上 |
| mutation 工具 | §11 只列出要植入的錯誤 | 完整執行時兩個案例被誤報為「存活」：一個因 trace 切開 libtest 的結果行，一個因植入的程式碼跟不上 S2 的修改而編譯失敗；另有一個錨點已失效。判定與錨點檢查已修正，見 [教學 04](lessons/04-mutation-checker-false-survivors/README.md) |
| 中斷點標記檔 | 未涉及 | 完整驗收時一個 S1 的 crash 測試偶發失敗：標記檔原地建立再寫入，測試在兩者之間讀到空字串。從 V1 就存在。改為先寫暫存檔再 rename，見 [教學 05](lessons/05-marker-seen-before-written/README.md) |
| 測試寫法與驗收程式 | 未涉及 | 一個 `#[should_panic]` 測試的結果行是 `... - should panic ... ok`，驗收程式只認 `... ok`，完整驗收到階段 11 才報錯。改用 `catch_unwind` 檢查 panic 訊息；新增 `scripts/preflight_phase.py`，開跑完整驗收前先用同一套規則跑該階段的測試 |
| mutation 證據的隔離（2026-11-13，S3 期間發現） | 未涉及 | S2 tag 所附的 mutation 報告是在所有案例共用一個 cargo 建置目錄下產生的，案例之間可能互相污染，其「全部抓到」的證據力較弱。S3 起每個案例獨立建置並先跑對照組；見 [教學 06](lessons/06-mutants-sharing-a-build-directory/README.md) |
| 模擬器的檢查範圍 | §10 列出 I6、I7 | 模擬器只檢查安全性，不檢查活性；拿掉 §9.4 的處理時模擬器仍全部通過，只有單元測試抓得到（教學 03 的練習） |
