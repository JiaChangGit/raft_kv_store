# WAL 格式 2 與持久化契約

更新日期：2026-11-12。這是目前程式實作的格式；實作在 `crates/raft-storage/src/lib.rs`。
較早的 [格式 1](STORAGE_FORMAT_V1.md) 只保留在 Git tag `v1-baseline-verified-20261112`，
本版程式不讀也不轉換格式 1 的資料。測試與驗收結果不寫在這裡，請看 `scripts/status.py` 與 receipt。

用語：**WAL**（write-ahead log）是節點把「任期、投給誰、log」寫進磁碟的檔案。
**殘尾**（torn tail）指程式寫一筆資料寫到一半被終止，檔案尾端留下的不完整內容。
**fail closed** 指拒絕啟動、不提供服務，而且不修改資料檔。**CRC** 是偵測意外毀損用的檢查碼。

## 1. 為什麼有格式 2

格式 1 的每筆 record 開頭有一個長度欄位，但這個欄位沒有自己的檢查碼。回復程式直接用它判斷
「這筆是不是沒寫完的殘尾」，是就把檔案截斷。於是長度欄位一旦壞成「比剩餘檔案還長」的數字，
這筆和它後面所有完好的 record 都會被當成殘尾刪掉。實測（2026-11-12）：

| 案例 | 格式 1 的結果 |
|---|---|
| A：第 1 筆長度 91→65536，之後以 term 1 寫入 | 啟動成功，檔案被清空重寫；原本 durable 的 term 是 2 |
| B：最後一筆長度 91→92 | 啟動成功，已 sync 的最後一筆被刪除 |

Raft 的安全性依賴節點不會忘記自己投過的票與已確認的 log（論文 Figure 2：這些狀態要在回覆 RPC 前
寫入穩定儲存）。完整的重現步驟與解說見 [Bug 教學 01](lessons/01-wal-length-corruption/README.md)。

## 2. 保證與不保證

**保證（有測試對應，見 §8）**

- G1：長度欄位通過 header 自己的檢查碼之前，不拿它做任何決定，也不依它配置記憶體。
- G2：只有兩種可辨識的殘尾會被截斷（§5.2）；截斷前先把那段位元組存成副本，副本沒存好就不截斷。
- G3：其餘任何不符，一律 fail closed，檔案一個位元組都不改。
- G4：WAL 檔自帶版本、節點與叢集身分，拿錯檔案會被拒絕。
- G5：第一次初始化在任何一步被終止，下次啟動都能接著完成；已用過的節點缺了 WAL 則拒絕啟動。
- G6：`save` 回傳成功代表 `sync_all` 已完成；任何寫入或同步錯誤後該 instance 不再接受寫入（poison）。
  回復只還原 term、vote、log，不決定哪些 entry 已 commit。

**不保證**

- N1：檔案被截短（不論是否剛好在兩筆 record 之間）、整檔遺失、或被較舊的完整檔取代。
  逐筆的檢查碼無法知道「後面本來還有幾筆」。截在兩筆之間會被當成較短的合法 log；
  截在一筆中間會被當成殘尾。整檔遺失則如 G5 拒絕啟動。
- N2：CRC32C 保證偵測單一位元錯誤與 32 bit 以內的連續錯誤；其他毀損約有 2⁻³² 的機率漏判。
  它不是密碼學驗證，擋不住蓄意竄改。
- N3：主機斷電或儲存裝置故障。測試只涵蓋 process 被 SIGKILL。斷電可能留下的全零區塊或亂碼
  在本格式會 fail closed，但「實際斷電後能否啟動」沒有實測。
- N4：自動修復。fail closed 之後怎麼處置由人決定。把節點資料清空再用同一個 ID 加回叢集，
  會讓它忘記投過的票，在 Raft 中並不安全。

## 3. 資料目錄

| 名稱 | 內容 |
|---|---|
| `node.lock` | 檔案鎖，程式執行期間持有，避免兩個程式同時寫同一個目錄 |
| `raft.wal` | WAL 本體：一個檔頭加上零到多筆 record |
| `node.meta` | JSON：`cluster_id`、`node_id`、`format_version`（2）。**它存在代表初始化已完成** |
| `torn/` | 回復時被截掉的殘尾副本（§5.3）。程式從不讀回，只供人查看 |
| `*.tmp` | 建立檔案時的暫存檔，不被當成資料 |

## 4. 位元組配置

整數皆為 little-endian。CRC32C：reflected Castagnoli 多項式 `0x82F63B78`，初值與結尾 XOR 皆
`0xFFFFFFFF`；ASCII `123456789` 的結果是 `0xE3069283`。

### 4.1 檔頭（80 bytes，offset 0）

| offset | bytes | 內容 | 本版的值 |
|---:|---:|---|---|
| 0 | 4 | magic | ASCII `RFTW` |
| 4 | 2 | 格式版本 | 2 |
| 6 | 2 | 檔頭長度 | 80 |
| 8 | 4 | `node_id` | 1～3 |
| 12 | 24 | `cluster_id` 的 UTF-8，尾端補 0 | `raft-kv-v1` |
| 36 | 4 | 保留 | 0 |
| 40 | 8 | `base_index`：這個檔案之前已被快照取代的最後一個 log index | 0 |
| 48 | 8 | `base_term`：該 index 的 term | 0 |
| 56 | 8 | `start_term`：檔案起點的 currentTerm | 0 |
| 64 | 8 | `first_sequence`：第一筆 record 的 sequence | 1 |
| 72 | 4 | `start_voted_for`：檔案起點的投票對象，0 代表沒有 | 0 |
| 76 | 4 | CRC32C，涵蓋 offset 0～75 | |

`cluster_id` 的值 `raft-kv-v1` 是叢集名稱，和儲存格式版本無關。

offset 40～75 的五個欄位記錄這個 WAL 從哪裡開始。全新節點是上表的值；做完快照後（S2 起），WAL 會被改寫成從快照之後開始的新檔，
這些欄位就記下快照的位置（`base_index`、`base_term`）、當時的 term 與投票，以及第一筆 record 的 sequence。
檔頭檢查不要求固定值，而是要求這些欄位彼此一致：保留位元組為 0、`base_index` 為 0 時 `base_term` 也是 0、`base_term` 不大於 `start_term`、
投票對象是 0～3 且有投票時 `start_term` 大於 0、`first_sequence` 至少為 1。它們和快照檔之間的一致性規則見
[快照規格](SNAPSHOT_COMPACTION.md) §7.3。這次格式變動沒有改外框，只是開始使用 S1 預留的欄位，因此格式版本仍是 2。

### 4.2 Record（從 offset 80 起連續排列）

| 相對 offset | bytes | 內容 |
|---:|---:|---|
| 0 | 4 | ASCII `RFTR` |
| 4 | 2 | 格式版本 = 2 |
| 6 | 2 | record 類型 = 1（Transaction） |
| 8 | 4 | `payload_len`：1 到 2,097,152（2 MiB） |
| 12 | 8 | `sequence`：從 1 起每筆加 1 |
| 20 | 4 | **`header_crc`**：CRC32C，涵蓋相對 offset 0～19 |
| 24 | `payload_len` | UTF-8 JSON 的 Transaction |
| 24+`payload_len` | 4 | `record_crc`：CRC32C，涵蓋 24 bytes 的 header 與整個 payload |

Transaction 的欄位依序是 `term`、`voted_for`、`truncate_from`、`append`，語意與格式 1 相同：
先把 log 從 `truncate_from` 起的尾段移除，再接上 `append`；term、vote 與 log 的變更在同一筆 record 內一起生效。
同一 term 內已 durable 的投票不可更換或清除；term 不可倒退；index 必須從 1 起連續。

之後若新增 log entry 的種類（例如成員設定變更），是在 JSON 內擴充，位元組外框不變；
舊程式遇到不認得的種類會解析失敗而 fail closed。

## 5. 開啟與回復

### 5.1 開啟目錄

先取得 `node.lock`，再依 `node.meta` 與 `raft.wal` 是否存在分成四種情況：

| `node.meta` | `raft.wal` | 處理 |
|---|---|---|
| 無 | 無 | 全新節點：先建立 `raft.wal`（只有檔頭），再建立 `node.meta`。兩者都是「寫暫存檔 → `sync_all` → rename → 同步目錄」 |
| 無 | 有 | 只有當檔案**恰好是本節點的全新檔頭、沒有任何 record** 時，視為上次初始化在最後一步前中斷，補寫 `node.meta`；否則拒絕 |
| 有 | 無 | 拒絕。初始化完成後 WAL 不見了是資料遺失，不能當成空白節點 |
| 有 | 有 | 檢查 `node.meta`，然後讀 WAL |

`node.meta` 最後才寫，所以它是「初始化已完成」的憑證。這讓初始化的每個中斷點都能接續：
還沒 rename 出 `raft.wal` 時目錄裡只有暫存檔（視為全新）；已有全新的 `raft.wal` 但沒有 `node.meta`
時補寫即可，因為這個節點從未成功開啟過，也就從未投票或確認任何資料。

`node.meta` 的檢查：大小不超過 4096 bytes、JSON 可解析且沒有多餘欄位、叢集與節點相符、
`format_version` 為 2。若為 1，錯誤訊息會指出這是格式 1 的資料；本版不會讀寫它的 `raft.wal`。

接著讀檔頭：檔案不足 80 bytes → 拒絕；先驗 CRC，再比對各欄位（§4.1）。

### 5.2 逐筆回復

從 offset 80 開始。設目前位置為 `o`，檔案剩餘 `r` bytes，預期的下一個 sequence 為 `n`。

| 情況 | 處理 |
|---|---|
| `r = 0` | 結束 |
| `r < 24`：header 不完整 | 已存在的每個位元組都要和「sequence 為 `n` 的 header 開頭」相符：magic、版本、類型、sequence 逐位元組比對；長度欄位若已完整須在 1～2 MiB 內；`header_crc` 若已出現一部分，須與前 20 bytes 算出的結果相符。全部相符 → **殘尾**；否則拒絕 |
| `r ≥ 24` | 依序檢查：① `header_crc` ② magic、版本、類型 ③ 長度範圍 ④ sequence 等於 `n`。任何一項失敗 → 拒絕 |
| header 通過，但 `r` 不足以容納整筆 record | **殘尾** |
| header 通過且 record 完整 | 讀 payload → 驗 `record_crc` → 解析 JSON → 套用並檢查狀態。任何一項失敗 → 拒絕。成功則前進到下一筆 |

全部讀完後再做一次 `sync_all`；失敗則 poison 並回傳錯誤。這是因為 crash 前「寫完但還沒 sync」的 record
可能存活下來，必須先確定它已落盤，才能讓上層把它當作可以據以回覆的基準。

為什麼截掉殘尾是安全的：`save` 要等整筆 record 寫完並 `sync_all` 成功才回傳，呼叫者也只在那之後才送出
投票或確認。process 在寫入途中被終止時，檔案是預定內容的一段開頭，所以不完整的最後一筆必然是
「從未回報成功」的那一筆。格式 1 的想法相同，差別在格式 2 先確認了「這是一筆 header 完好、只是沒寫完的 record」，
才把它當殘尾。

### 5.3 殘尾的保留與截斷

1. 把 `o` 到檔尾的位元組寫到 `torn/seq<補零到 20 位的 n>-<這段位元組的 CRC32C，8 位小寫十六進位>.bin`
   （暫存檔 → `sync_all` → rename）。內容相同則檔名相同，重做不會多出一份。
2. `torn/` 內符合上述命名的檔案最多留 8 個（`MAX_TORN_COPIES`）；超過時刪除檔名排序最前面的，
   但永遠不刪剛寫入的那一個。sequence 不同時，排在前面的就是最舊的；sequence 相同時次序由 CRC 決定，
   不代表時間先後。先前中斷留下的 `.bin.tmp` 會順手清掉。其他名稱的檔案不計數也不刪除。
3. 以上都成功後，才 `set_len(o)` 並 `sync_all`。**任何一步失敗就不截斷，直接拒絕啟動。**

單一副本小於 2 MiB 加 28 bytes，`torn/` 總量因此有上界（約 16 MiB）。

### 5.4 寫入

序列化並檢查 payload 上限 → `write_all` → `sync_all` → 更新記憶體中的 durable 狀態與 sequence → 回傳成功。
`write_all` 或 `sync_all` 失敗就 poison。呼叫者只有在成功回傳後才解除等待、計入本機進度或送出確認；
儲存層不決定 commit，也不產生 RPC。

目前每次 `save` 仍會比對並複製整份 log。這是已知的效能問題；因為更大的成本在 core 每個事件都複製整份狀態，
兩者會在 S2 重構 log 結構時一起處理。

## 6. 各情境的結果

| 情境 | 結果 |
|---|---|
| 正常停止後重開 | 成功，狀態完整 |
| SIGKILL：寫入前 | 先前的 durable 狀態 |
| SIGKILL：只寫出一部分 header 或一部分 payload | 殘尾存成副本後截斷，成功啟動，狀態為前一筆 |
| SIGKILL：寫完尚未 sync | 舊狀態或完整新交易皆可；不能要求未 sync 的交易一定消失 |
| SIGKILL：sync 後尚未回報 | 新交易存在，但用戶端或其他節點可能還不知道 |
| SIGKILL：初始化的任何一步 | 下次啟動完成初始化 |
| 任一 record 的 header 任一位元毀損（含長度欄位、含最後一筆） | 拒絕，檔案不變 |
| payload 或 `record_crc` 毀損 | 拒絕，檔案不變 |
| 檔頭毀損、節點或叢集不符、預留欄位非本版的值 | 拒絕，檔案不變 |
| sequence 為 0、重複或跳號；長度為 0 或超過上限；未知的版本或類型；格式 1 的 record | 拒絕，檔案不變 |
| 尾端附加亂碼或全零 | 拒絕，檔案不變 |
| 殘尾副本寫不進去 | 拒絕，不截斷 |
| `write_all` 或 `sync_all` 失敗 | 不回成功，instance poison |
| 格式 1 的資料目錄 | 拒絕，目錄不變 |
| 檔案在兩筆 record 之間被截短 | **偵測不到**：成功啟動，狀態較舊（N1） |
| 檔案在某筆 record 中間被截短 | **與殘尾無法區分**：該段存成副本後截斷（N1） |

## 7. 測試用的中斷點

環境變數 `RAFT_WAL_FAILPOINT`（搭配 `RAFT_WAL_FAILPOINT_MARKER`）是明確選用的測試機制：程式到達指定位置後
寫下標記並等待，由測試對它送 SIGKILL。正常執行不會設定這些變數。可用的位置：

| 名稱 | 位置 |
|---|---|
| `before_write`、`after_write_before_sync`、`after_sync_before_ack` | `save` 的三個邊界 |
| `after_partial_header`、`after_partial_payload` | 只寫出 record 的前 13 bytes，或 header 加上一半 payload |
| `init_before_rename_raft.wal`、`init_after_wal_before_identity`、`init_before_rename_node.meta` | 初始化的三個邊界 |
| `install_before_snapshot_rename`、`install_after_snapshot_before_wal`、`install_before_wal_rename`、`install_after_wal_rename` | 安裝快照的四個邊界（S2，`snapshot_probe` 搭配使用） |

`wal_probe` 是搭配這些中斷點使用的小程式，不是共識實作。

## 8. 對應的測試

契約 `acceptance/final.json` 的階段 10 列出完整清單。對照 §2：

| 保證 | 測試 |
|---|---|
| G1、G3 | `reproduced_length_corruption_fails_closed_unchanged`（案例 A、B）、`every_single_bit_flip_fails_closed_unchanged`（逐一翻轉整個檔案的每個位元）、`seeded_multibyte_corruption_never_accepts_or_shortens`（固定種子 10,000 次）、`header_field_rules_checked_after_crc`、`zero_or_garbage_tail_rejected`、`partial_header_tail_checks_every_available_byte` |
| G2 | `truncated_wal_tail_safe`（最後一筆的每個切點）、`torn_tail_copy_failure_blocks_truncation`、`torn_tail_copies_are_idempotent_and_bounded`、`real_process_partial_write_is_preserved_then_truncated` |
| G4 | `new_directory_layout_and_file_header`、`file_header_corruption_and_identity_mismatch_rejected`、`file_header_start_fields_validated`（S2 取代原本的 `reserved_header_fields_must_be_pristine`）、`v1_directory_rejected_without_modification` |
| G5 | `interrupted_initialization_is_completed_or_rejected`、`existing_identity_without_wal_fails_closed`、`real_process_crash_during_initialization_recovers` |
| G6 | `append_ack_after_durable_sync`、`vote_after_restart_not_duplicated`、`recovered_prefix_requires_successful_sync`、`uncommitted_suffix_not_applied_after_recovery`、`real_process_crash_boundaries_restore_only_complete_transactions` |
| 叢集層面 | `test_corrupt_header_node_refuses_start_and_cluster_continues`（三個真實 process） |
| N1 | `boundary_truncation_is_documented_as_undetected`：把這項不保證固定下來，日後行為若改變會被發現 |

`scripts/mutation_check.py` 另在隔離副本裡逐一植入十個儲存層的錯誤（例如略過 `header_crc`、
先比對檔尾再驗檢查碼、先截斷再存副本），要求上表中指定的測試失敗。
