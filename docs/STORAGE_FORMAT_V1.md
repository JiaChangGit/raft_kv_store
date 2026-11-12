# WAL 格式 v1 與持久化契約

> **歷史文件。** 目前的程式使用 [格式 2](STORAGE_FORMAT_V2.md)，不再讀寫格式 1；格式 1 的程式保存在
> Git tag `v1-baseline-verified-20261112`。下文保留原樣，其中「限制」一段描述的問題就是 [Bug 教學 01](lessons/01-wal-length-corruption/README.md) 的主題。

此文件描述本專案實作，不是遺失的原始 gate。適用 Linux／WSL ext4。
每個節點使用獨立目錄；`node.lock` 的標準庫檔案鎖由 Wal 全生命週期持有，
防止相同資料目錄有兩個 writer。`node.meta` 以 JSON 保存 cluster_id、node_id、
format_version；錯誤、部分內容或不符的身分皆拒絕開啟。

新目錄逐層建立後同步父目錄。node.meta 以 create_new 寫入並 sync_all，
再同步資料目錄；raft.wal 初次建立也同步檔案與資料目錄。建立中斷造成不完整
metadata 會保持不可用，不冒用舊 WAL。既有 WAL 缺少 identity 時拒絕。
已有 identity 卻缺 WAL 也拒絕，即使可能是第一次初始化中斷，也不猜測它是空白節點。
回復接受完整 prefix 後會再次 sync_all；成功後才允許 Core 將它當成 durable 基線。
回復同步失敗即不提供服務，避免未同步但倖存的 record 被直接當作成功 ACK 的依據。

| offset | bytes | 語意 |
|---|---:|---|
| 0 | 4 | ASCII RFT1 |
| 4 | 2 | little-endian version=1 |
| 6 | 2 | little-endian record_type=1（Transaction） |
| 8 | 4 | little-endian payload_len，1～2 MiB |
| 12 | 8 | little-endian sequence，從 1 連續增加 |
| 20 | payload_len | UTF-8 JSON Transaction |
| 20+payload_len | 4 | little-endian CRC32C，涵蓋 header+payload |

Transaction 欄位順序由 Rust struct 固定：term、voted_for、truncate_from、append。
truncate_from 從該 index 移除既有 suffix，再加入 append；兩者包含在同一筆 record。
每次 save 比對完整持久狀態，只編碼共同 prefix 之後的變更。hard state 與 log 變更
在一筆交易內共同回復。sentinel index 0 不寫入 WAL；真實 log 必須從 1 無缺口。
同一 term 的 durable vote 不可更換或清空；term 不可倒退。

CRC32C 使用 reflected Castagnoli polynomial `0x82F63B78`，initial/final XOR
皆 `0xFFFFFFFF`；ASCII `123456789` 的 checksum 是 `0xE3069283`。
CRC 是意外損壞偵測，不是密碼學驗證或任意損壞恢復保證。

save 順序是 serialize/bounds → write_all → sync_all → 更新記憶體 durable state／sequence
→ 返回成功。任何 write/sync 錯誤將該 Wal instance poison；後續 save 一律拒絕，
必須停止依賴效果並重啟回復。呼叫者只在成功返回後解除 barrier、計入 local match
或送 durable ACK；儲存層不決定 commit，不產生 RPC。

回復逐筆有界讀取，檢查 magic、version、type、長度、連續 sequence、CRC 與交易後
狀態。只允許尾端可辨識的部分 header 或合法 header 所指的未完整 payload/checksum，
將檔案截到上一完整 record，並 sync_all。任意 garbage、完整 record CRC 錯誤、
未知格式、非法索引及可辨識的 CRC 毀損會 fail closed。截尾若再 crash，下一次回復重新
執行相同判定；沒有 in-place 改寫已驗證 prefix。

限制：v1 沒有獨立 header checksum。若任意磁碟毀損把中間 record 的 payload_len
改成範圍內、卻超過 EOF 的數字，它可能與合法的未完成尾端無法區分，並觸發截尾。
因此不能宣稱 v1 能辨識所有中間毀損。正常 crash 的 append-prefix 模型與任意位元毀損
是不同條件；後者不在完整保證內。改善此限制需要有相容性與遷移設計的新格式，留待 V2。

回復結果只有 term/vote/log。runtime 必須建立 Follower、commitIndex=lastApplied=0、
空 KV/session，重新獲得合法 committed prefix 後依序 apply。

| crash boundary | 允許的結果 |
|---|---|
| before_write | 先前 durable state |
| after_write_before_sync | 舊狀態或完整新交易；不可要求未同步交易必定消失 |
| after_sync_before_ack | 新交易仍存在，但 client／peer 尚可能不知道 |
| partial trailing record | 截除不完整尾端，不發明 entry |
| complete CRC corruption | 拒絕啟動 |
| write/sync error | 不回成功；instance poison |

`wal_probe` 搭配 RAFT_WAL_FAILPOINT 和 RAFT_WAL_FAILPOINT_MARKER 是明確 opt-in
process crash fixture：到邊界寫 marker 後等待，由測試對子程序 SIGKILL；不是正式
共識實作。`real_process_crash_boundaries_restore_only_complete_transactions` 實際
啟動、殺死並重開儲存子程序。這些測試不證明任意主機斷電或裝置故障保證。

執行結果由階段證據另行記錄；此文件不將尚未執行的測試標示為通過。
