# WAL 格式 v2 與遷移規格（草稿）

> **2026-11-12 註記：本草稿的前提已改變。** 使用者確認只保留新格式，不做新舊並存與遷移
> （見 [ROADMAP.md](ROADMAP.md)）。§7、§8、D1、D2、D4 不再適用，全文將在 S1 階段重寫；
> §4～§6 的格式與回復設計仍是重寫的基礎。

狀態：**草稿，待使用者審閱；尚未實作，本文件沒有任何一項已被測試驗證。**
更新日期：2026-11-12。v1 的契約與已知限制見 [STORAGE_FORMAT_V1.md](STORAGE_FORMAT_V1.md)，該文件保留原文。
驗收條件見 `acceptance/v2.json`（同為草稿）。

用語：**殘尾**（torn tail）指程式在寫入一筆 record 的途中被中斷，檔案尾端留下的不完整內容。
**fail closed** 指拒絕啟動、不提供服務，且不修改資料檔。**CRC** 是用來偵測意外毀損的檢查碼。

## 1. 要解決的問題

v1 的 record header 沒有自己的檢查碼。回復程式在確認 header 可信之前，就用其中的
`payload_len` 判斷「這筆是不是殘尾」，是就截斷檔案。2026-11-12 以既有 `wal_probe` 對副本實測：

| 案例 | 結果 |
|---|---|
| A：第 1 筆長度 91→65536，再以 term 1 寫入 | 啟動成功，檔案被清空重寫；原 durable term 為 2，term 倒退、投票與兩筆 log 全失 |
| B：最後一筆長度 91→92 | 啟動成功，已 sync 的最後一筆被當成殘尾刪除 |

節點忘記已投的票與已 ACK 的 log，破壞的是 Raft 安全性的前提，不只是可用性。
歷史證據在 `artifacts/audit/wal-review-20261110/`；A、B 的正式重現證據將在實作階段的第一步存入 `artifacts/audit/`。

## 2. 目標與不保證事項

**目標**

- G1：決定「截斷」之前，長度必須已通過檢查碼驗證。任何完整但驗證失敗的 header 一律 fail closed。
- G2：除了已驗證的殘尾之外，回復程式不修改 WAL；截斷殘尾前先保留其副本。
- G3：WAL 檔自帶版本與節點身分，拿錯檔案或錯版本會被拒絕。
- G4：v1 資料只經由使用者明確執行的離線工具轉換；工具不刪除、不覆寫任何既有的非暫存檔，
  任一步驟中斷後重跑都會收斂。
- G5：`save` 的成功語意、poison 行為、回復後 `sync_all`、回復只還原 term/vote/log，全部與 v1 相同。

**不保證（須在文件與回報中明講）**

- N1：檔案被截短（不論是否剛好在 record 邊界）、整檔遺失、或被較舊的完整檔取代。逐筆檢查碼無法偵測
  「少了後面幾筆」；整檔遺失仍如 v1 一樣 fail closed。
- N2：CRC32C 保證偵測單一位元錯誤與長度不超過 32 bit 的連續錯誤；其他毀損型態約有 2⁻³² 的機率漏判。
  它不是密碼學驗證，無法防範蓄意竄改。
- N3：主機斷電或裝置故障。測試只涵蓋 process 被 SIGKILL；斷電可能留下的全零區塊或亂碼，
  在 v2 會 fail closed（見 §5），但「實際斷電後的可用性」沒有實測。
- N4：v1 檔案過去是否曾被誤截斷。遷移前的檢查只能確認檔案現在自洽。
- N5：自動修復毀損。fail closed 之後如何處置節點由人決定；本階段不提供修復工具。
  注意：把節點資料清空後以同一個 ID 重新加入，會讓它忘記投過的票，在 Raft 中並不安全。

## 3. 資料目錄

| 檔名 | 內容 |
|---|---|
| `node.lock` | 檔案鎖，節點程式與遷移工具執行期間都持有；同 v1 |
| `node.meta` | JSON：`cluster_id`、`node_id`、`format_version`。**`format_version` 決定哪個 WAL 檔是權威資料** |
| `raft.v2.wal` | v2 WAL（`format_version` 為 2 時的權威檔） |
| `raft.wal` | v1 WAL（`format_version` 為 1 時的權威檔） |
| `torn/` | 回復時保留的殘尾副本（§5.3）；程式從不讀取其內容 |
| `migration-archive/<UTC時間>-<序號>/` | 遷移工具的備份與被取代的檔案（§7）；程式從不讀取其內容 |
| `*.tmp` | 暫存檔，隨時可被重建；任何程式都不把它當成資料 |

`cluster_id` 的值 `raft-kv-v1` 是叢集名稱，與儲存格式版本無關，v2 不更動。

## 4. 位元組配置

所有整數皆為 little-endian。CRC32C 的定義與 v1 相同（reflected Castagnoli `0x82F63B78`，
初值與結尾 XOR 皆 `0xFFFFFFFF`，`123456789` 的結果為 `0xE3069283`）。

### 4.1 檔頭（file header，40 bytes，位於 offset 0）

| offset | bytes | 內容 |
|---|---:|---|
| 0 | 4 | ASCII `RFTW` |
| 4 | 2 | `format_version` = 2 |
| 6 | 2 | 檔頭長度 = 40 |
| 8 | 1 | `node_id`（1～3） |
| 9 | 3 | 保留，必須為 0 |
| 12 | 24 | `cluster_id` 的 UTF-8，尾端以 0 補滿；必須等於 `raft-kv-v1` |
| 36 | 4 | CRC32C，涵蓋 offset 0～35 |

### 4.2 Record（從 offset 40 起連續排列）

| 相對 offset | bytes | 內容 |
|---|---:|---|
| 0 | 4 | ASCII `RFT2` |
| 4 | 2 | `version` = 2 |
| 6 | 2 | `record_type` = 1（Transaction） |
| 8 | 4 | `payload_len`，1～2 MiB（2,097,152） |
| 12 | 8 | `sequence`，從 1 連續遞增 |
| 20 | 4 | **`header_crc`**：CRC32C，涵蓋相對 offset 0～19 |
| 24 | `payload_len` | UTF-8 JSON Transaction，**內容與 v1 完全相同** |
| 24+`payload_len` | 4 | `record_crc`：CRC32C，涵蓋 24 bytes header 與 payload |

與 v1 的差別只有三點：magic 與 version 改為 2、header 多 4 bytes 的 `header_crc`、
`record_crc` 的涵蓋範圍隨 header 變長。Transaction 的欄位（`term`、`voted_for`、`truncate_from`、
`append`）、語意與 sequence 編號都不變，因此 v1 與 v2 之間可以逐筆原樣搬移 payload，不需重新序列化。

## 5. 開啟與回復

### 5.1 開啟目錄

依序檢查，任何一項不符即 fail closed，且不建立、不修改任何檔案（取得 `node.lock` 除外）：

1. `node.meta` 與兩個 WAL 檔都不存在 → 視為全新節點：以「寫暫存檔、`sync_all`、rename、同步目錄」
   依序建立 `node.meta`（`format_version` 2）與只含檔頭的 `raft.v2.wal`。
   此時殘留的 `*.tmp` 可直接覆寫。
2. `node.meta` 存在但無法解析、過大、身分不符 → 拒絕（同 v1）。
3. `format_version` 為 1 → 拒絕，錯誤訊息指出這是 v1 資料並提示執行 `wal_migrate to-v2`。**不讀寫 `raft.wal`。**
4. `format_version` 為 2 但 `raft.v2.wal` 不存在 → 拒絕（同 v1 的「有身分卻缺 WAL」，不猜測是空白節點）。
5. 沒有 `node.meta` 卻有任一 WAL 檔 → 拒絕（同 v1）。
6. `format_version` 為 2 且同時留有 `raft.wal` → **照常啟動，並在 stderr 警告**有一個非權威的舊檔，
   提示重跑 `wal_migrate to-v2` 完成歸檔。（待確認 D2）
7. 檔頭：檔案不足 40 bytes、CRC 不符、任何欄位與預期或 `node.meta` 不符 → 拒絕。
   因為檔頭一律經暫存檔 rename 建立，正常中斷不會留下不完整的檔頭。

初始化若在兩個檔案之間中斷，會落在第 4 項而保持不可用，與 v1 相同；這個節點從未成功開啟，
也就從未投票或 ACK。自動續做初始化不在本階段範圍。

### 5.2 逐筆回復

令目前位置為 `o`、剩餘長度 `r = 檔案長度 − o`、預期的下一個 sequence 為 `n`。

| 條件 | 處理 |
|---|---|
| `r = 0` | 結束 |
| `r < 24`（header 不完整） | 已存在的位元組中，magic、version、type、sequence 各欄位必須與預期值逐位元組相同；若長度欄位已完整（`r ≥ 12`）必須在 1～2 MiB 內。全部符合 → **殘尾**；否則拒絕 |
| `r ≥ 24` | 依下列次序檢查，任何一項失敗即**拒絕且不改檔**：① `header_crc` ② magic、version、type ③ 長度範圍 ④ sequence 等於 `n` |
| header 通過，且 `r < 24 + payload_len + 4` | **殘尾** |
| header 通過，record 完整 | 讀入 payload → `record_crc` → 解析 JSON → 套用交易並檢查狀態（規則同 v1）；任何一項失敗即拒絕。成功則 `o` 前進、`n` 加 1 |

要點：

- 在 `header_crc` 通過之前，不使用 `payload_len` 做任何決定，也不依它配置記憶體。
- 殘尾只有兩種型態，其餘一律拒絕。全零或亂碼的尾端不符合任何一種，因此會拒絕。
- 迴圈結束後執行 `sync_all`，失敗則 poison 並回傳錯誤（同 v1）。

為什麼這樣判定殘尾是安全的：`save` 只有在整筆 record 寫完並 `sync_all` 成功後才回傳成功，
呼叫者也只在那之後才送出投票或 ACK。process 在寫入途中被終止時，檔案是預定內容的一段前綴，
所以不完整的最後一筆必然是「從未回報成功」的那一筆。v1 的判斷相同，差別在 v2 先確認了
「這是一筆 header 完好、只是還沒寫完的 record」，而不是「一筆長度被改壞的完整 record」。

### 5.3 殘尾的保留與截斷

1. 將 `o` 到檔尾的位元組寫入 `torn/seq<20 位數補零的 n>-<這段位元組的 CRC32C，8 位十六進位>.bin`
   （先寫暫存檔、`sync_all`、rename、同步 `torn/` 目錄）。內容相同則檔名相同，重跑不會產生第二份。
2. 第 1 步成功後才 `set_len(o)` 並 `sync_all`。**保留失敗就不截斷，直接拒絕啟動。**
3. `torn/` 最多保留 8 個檔案；超過時刪除檔名排序最前面（sequence 最小）的。單檔上限約 2 MiB，
   所以總量約 16 MiB。（上限值待確認 D3）

截斷後若再次中斷，下一次回復重新執行相同判定。

### 5.4 寫入

與 v1 相同：序列化並檢查上限 → `write_all` → `sync_all` → 更新記憶體中的 durable 狀態與 sequence →
回傳成功。任何 write 或 sync 錯誤使該 instance poison，之後的 `save` 一律拒絕。
既有的三個 failpoint 保留；另增測試專用的 `after_partial_write`（只寫入 record 的前半段後等待被終止），
用來以真實 process 產生殘尾。

## 6. 各情境的預期結果

| 情境 | 預期 |
|---|---|
| 正常停止後重開 | 成功，狀態完整 |
| SIGKILL：寫入前 | 先前的 durable 狀態 |
| SIGKILL：寫了一部分 | 保留殘尾副本、截斷、成功啟動；狀態為前一筆 |
| SIGKILL：寫完尚未 sync | 舊狀態或完整新交易皆可（同 v1） |
| SIGKILL：sync 後尚未回報 | 新交易存在 |
| 任一 record 的 header 任一位元毀損（含最後一筆、含長度欄位） | 拒絕，檔案不變 |
| payload 或 `record_crc` 毀損 | 拒絕，檔案不變 |
| 檔頭毀損、節點或叢集身分不符 | 拒絕，檔案不變 |
| sequence 重複或跳號、長度為 0 或超過上限、未知 version／type、v2 檔中出現 v1 record | 拒絕，檔案不變 |
| 尾端附加亂碼或全零 | 拒絕，檔案不變 |
| write 或 sync 失敗 | 不回成功，instance poison |
| 殘尾副本寫入失敗 | 拒絕，不截斷 |
| 檔案在 record 邊界被截短 | **無法偵測**：成功啟動，狀態較舊（N1） |
| 檔案在某筆 record 中間被截短 | **無法與殘尾區分**：該筆被保留副本後截斷（N1） |
| v2 程式遇到 v1 目錄 | 拒絕並提示遷移，目錄不變 |
| v1 程式遇到 v2 目錄 | 拒絕（既有行為：`format_version` 不符） |

## 7. v1 與 v2 之間的遷移

### 7.1 原則

- 只有使用者明確執行 `wal_migrate` 才會轉換；節點程式從不自動轉換。
- 工具全程持有 `node.lock`；節點執行中時工具會拒絕。
- **工具不刪除、不覆寫任何非 `.tmp` 檔案。** 需要讓位的檔案一律以 rename 移入
  `migration-archive/<UTC時間>-<序號>/`。這個目錄由使用者自行決定何時清理。
- 轉換時逐筆原樣搬移 payload 位元組與 sequence；因此 v1 → v2 → v1 的結果與原檔逐位元組相同
  （前提是原檔沒有殘尾）。
- 以「重讀後逐位元組比對」驗證備份與新檔，不依賴雜湊函式，也不新增相依套件。

### 7.2 命令

| 命令 | 作用 |
|---|---|
| `wal_migrate check <目錄> <node_id>` | 唯讀檢查並輸出 JSON 報告：權威格式、record 數、最後 sequence、term、voted_for、last index、尾端狀態、各檔長度與 CRC32C、目錄處於 §7.4 的哪一種形態。不建立任何檔案 |
| `wal_migrate to-v2 <目錄> <node_id> [--accept-torn-tail]` | 讓目錄收斂到 v2 |
| `wal_migrate to-v1 <目錄> <node_id>` | 讓目錄收斂到 v1（即 rollback） |

結束碼：0 成功或已是目標狀態；2 因資料狀況拒絕（未做任何變更）；1 I/O 或其他錯誤。

### 7.3 來源檔的嚴格檢查

轉換前以唯讀方式掃描權威檔，全部 CRC、sequence 與交易狀態都必須通過。

- 來源為 v2：必須結束在 record 邊界。有殘尾 → 拒絕，請先以節點程式正常開啟一次（依 §5.3 處理）。
- 來源為 v1，尾端依 v1 規則屬於「不完整的最後一筆」時：
  - 先在這段尾端內搜尋：是否存在另一個自洽的 v1 header（magic、version、type 正確，長度在範圍內，
    且 sequence 大於預期值）。**若有，判定為毀損並拒絕，`--accept-torn-tail` 也無效。**
    理由：payload 是 UTF-8 JSON，控制字元都會被跳脫，不可能含有 header 中的原始 0 位元組；
    所以真正的殘尾裡不會出現另一個 header，出現了就代表這不是殘尾，而是 §1 的案例 A。
  - 若沒有：預設仍拒絕並在報告中列出尾端位置、長度與宣告的長度。使用者看過後加上
    `--accept-torn-tail` 才繼續；這段尾端不會被搬進 v2，原檔會原封不動進入歸檔。
    工具無法分辨這是真正的殘尾還是案例 B，因此必須由人決定。（N4）

### 7.4 目錄形態與收斂

以 M 表示 `node.meta` 的 `format_version`。規則只有一條：**M 指向的 WAL 是權威檔，另一個 WAL 檔若存在即為非權威，
從不被信任，只會被歸檔。**

| 形態 | 內容 | 節點程式 | `to-v2` | `to-v1` |
|---|---|---|---|---|
| ① | M=1，`raft.wal` | v1 程式可執行；v2 程式拒絕 | 執行轉換 | 已完成 |
| ② | M=1，`raft.wal`，`raft.v2.wal` | 同 ① | 歸檔 `raft.v2.wal` 後執行轉換 | 歸檔 `raft.v2.wal` |
| ③ | M=2，`raft.wal`，`raft.v2.wal` | v2 程式警告後執行；v1 程式拒絕 | 歸檔 `raft.wal` | 歸檔 `raft.wal` 後執行轉換 |
| ④ | M=2，`raft.v2.wal` | v2 程式可執行；v1 程式拒絕 | 已完成 | 執行轉換 |

其他組合（例如 M 指向的檔案不存在）→ 工具與節點程式都拒絕。

`to-v2` 的轉換步驟（`to-v1` 完全對稱，把 1 與 2、兩個檔名互換）：

1. 取得鎖，辨識形態；若有非權威的 WAL 檔，移入新的歸檔目錄。
2. 嚴格檢查 `raft.wal`（§7.3）。不通過 → 結束碼 2。
3. **備份**：把 `raft.wal` 與 `node.meta` 複製到歸檔目錄，`sync_all` 檔案與目錄，重讀並逐位元組比對。
4. **建立新檔**：寫 `raft.v2.wal.tmp`、`sync_all`；用 v2 的嚴格讀取器重讀，確認 record 數、每筆 sequence、
   每筆 payload 位元組，以及最終的 term、voted_for、log 都與來源相同。
5. rename 為 `raft.v2.wal`，同步目錄。（此時為形態 ②，仍是 v1 節點）
6. **切換**：寫 `node.meta.tmp`（`format_version` 2）、`sync_all`、rename 為 `node.meta`、同步目錄。
   **這一次 rename 是唯一的切換點。**（此時為形態 ③）
7. 把 `raft.wal` 移入歸檔目錄，同步兩個目錄。（形態 ④）

任何一步中斷後重跑同一個命令：切換點之前，一切從頭重做（先前產生的非權威檔被歸檔，不會被沿用，
因為 v1 節點可能在這期間又寫入了新 record）；切換點之後，只剩歸檔要完成。

### 7.5 Rollback

唯一支援的 rollback 是 `wal_migrate to-v1`。它把 v2 目前的全部內容（包含切換後新寫入的 record）
原樣轉回 v1 格式，所以不會遺失投票或 log。

**不要手動把歸檔裡的舊 `raft.wal` 搬回來。** 只要 v2 節點在切換後寫過任何 record，
舊檔就少了那些投票與 log，搬回來等於重演 §1 的危害。`check` 的報告會列出最後 sequence 供人工比對。

### 7.6 叢集層面

儲存格式是各節點本機的事，網路協定不變；推論上可以一次停一台、轉換、重啟，維持 quorum 2。
**這是推論。** 驗收中的三程序測試只用新版程式（把運作中的 v2 節點轉成 v1 再轉回 v2），
沒有讓舊版與新版執行檔同時組成叢集；後者列為未驗證。

## 8. 程式結構與 v1 驗收的關係

- 現有的 v1 讀寫程式移入 `raft_storage::v1` 模組並**凍結**：除了搬移位置、調整可見性、
  以及新增一個唯讀掃描函式（供 §7.3 使用）之外，不修改邏輯。§1 的缺陷在 v1 模組中**不修正**——
  修了就不再是 v1，也無法讀回既有資料時的原始行為。節點程式不再呼叫 v1 的寫入路徑。
- `raft_storage::Wal` 成為 v2 實作，對外介面（`open`、`save`、`state`、`durable_sequence`、
  `poisoned`、`inject_sync_failure` 等）不變，`raft-node` 與 `wal_probe` 不需修改呼叫方式。
- `acceptance/v1.json` 不修改。其中 P03 的四個儲存測試會隨 v1 模組搬移，測試內容不動，
  **之後它們驗證的是凍結的 v1 模組，不是節點實際使用的 v2 路徑。** 為了不讓驗收變弱，
  `acceptance/v2.json` 為這四項性質各訂一個針對 v2 的對應測試。（待確認 D1）
  P04 之後的三程序測試與既有的真實 process crash 測試經由 `Wal::open` 執行，會自動改為驗證 v2。
- 取自歷史證據的 230 bytes v1 檔（兩筆 record）將放入 `crates/raft-storage/tests/fixtures/`，
  作為由當時的 v1 程式實際產生的固定測試輸入。

## 9. 驗收摘要

完整清單、測試名稱與各階段條件見 `acceptance/v2.json`。重點：

1. 先存下案例 A、B 的正式重現證據與會失敗的測試（保留首敗），再開始實作。
2. 對一個含多筆 record 的小檔，**逐一翻轉每一個位元**，每次都必須拒絕啟動且檔案不變。
3. 最後一筆 record 內的每一個截斷點都必須回到前一筆狀態，且 `torn/` 內有內容相符的副本。
4. 固定種子的 10,000 次多位元組毀損，不得出現「啟動成功」或「檔案被改短」。
5. 真實 process 在 `save` 的四個 failpoint 與遷移的每個步驟後被 SIGKILL，重啟或重跑後符合 §6、§7.4。
6. 遷移前後盤點：原本存在的每個非暫存檔，其位元組內容之後仍存在於目錄樹的某處。
7. 三程序叢集：其中一台的 header 被毀損後拒絕啟動，另外兩台繼續服務且歷史可線性化。
8. 在隔離副本植入三個錯誤（略過 `header_crc`、驗證失敗時改為截斷、先比對檔尾再驗證），測試必須抓到。
9. 最後在同一份來源上，從 P00 重跑 v1 驗收，再跑 v2 驗收與乾淨副本重建。

## 10. 待確認事項

| 編號 | 問題 | 建議 |
|---|---|---|
| D1 | v1 程式凍結保留，P03 四測試改為驗證 v1 模組，v2 另訂對應測試 | 採用；否則就得修改 v1 驗收或放棄檔頭 |
| D2 | `format_version` 為 2 但留有舊 `raft.wal` 時，v2 節點要不要啟動 | 警告後啟動：這個檔案不影響資料正確性，拒絕只會讓「切換後、歸檔前」的中斷多一次人工介入 |
| D3 | `torn/` 保留上限 | 8 個檔案（約 16 MiB） |
| D4 | 執行 v2 契約需讓 `scripts/verify_gate.py` 能指定契約檔；checker 的 hash 會改變，須從 P00 重驗 | 採用；完成時本來就要從 P00 重跑 |
