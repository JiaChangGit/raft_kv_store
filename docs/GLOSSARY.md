# 名詞與縮寫

這份清單收錄文件裡出現的專有名詞與縮寫。每一條寫兩件事：白話的意思（能舉例的就附一個例子），以及它在這個專案的哪裡。
英文保留原文，因為程式、論文與錯誤訊息都用原文；第一次讀時可以先看「白話」那一欄。

數字（上限、逾時）取自 2026-11-17 的程式；改程式時請一起更新。

## Raft 的基本概念

| 名詞 | 白話 | 在這個專案 |
|---|---|---|
| Raft | 一種共識演算法：讓幾台機器對「資料依什麼順序更新」取得一致。多數機器活著就能繼續寫入。論文：Ongaro 與 Ousterhout，2014 | `crates/raft-core` |
| 共識（consensus） | 多台機器對同一件事得到同一個答案，即使有機器當機或訊息遺失 | — |
| 節點（node） | 叢集裡的一台機器；這裡是一個 `raft-node` process | 編號 1～7 |
| leader／follower／candidate | 三種角色。leader 接受寫入並複製給其他人；follower 照 leader 的 log 更新；candidate 是正在拉票的節點 | `Role`（`raft-core`） |
| term（任期） | 一個遞增的數字，每次選舉加一。同一個 term 最多一個 leader。看到更大的 term，就知道自己過時了 | `DurableState::term` |
| 選舉（election） | follower 太久沒收到 leader 的消息，就把 term 加一、變成 candidate，向其他節點要票。拿到多數票就成為 leader | 選舉逾時 400～800 毫秒（隨機） |
| 心跳（heartbeat） | leader 定期送出的 AppendEntries，沒有新資料也送，告訴其他人「我還在」 | 每 100 毫秒 |
| log 與 entry | log 是一串依序編號的命令，每一筆叫 entry，帶著寫下它的 term。所有節點最後會有相同的 log | `LogEntry` |
| index | entry 在 log 裡的位置，從 1 開始 | `LogIndex`；上限 `MAX_LOG_INDEX`（2^62） |
| 多數（majority、quorum） | 超過一半的 voter。三台要兩台，五台要三台，一台要一台 | `Voters::majority` |
| commit（提交） | 一筆 entry 已經在多數節點落盤，而且不會再被改掉。commit 之後才能套用 | `commit_index` |
| apply（套用） | 依 index 順序，把已 commit 的命令真正執行在狀態機上，例如把 x 設成 A | `Effect::Apply`、`StateMachine::apply` |
| 狀態機（state machine） | 執行命令的那個東西。這裡是一個 key-value 對照表 | `crates/kv-state` |
| NoOp | 新 leader 當選後立刻寫入的一筆空命令。它被 commit，代表 leader 已經知道之前 commit 到哪裡 | `Command::NoOp` |
| RPC（Remote Procedure Call，遠端程序呼叫） | 一台機器請另一台做事並等回覆。Raft 的基本兩種是 RequestVote（要票）與 AppendEntries（複製 entry）；這個專案另有 InstallSnapshot（送快照） | `Rpc`（`raft-protocol`） |
| Figure 8 | Raft 論文的一張圖：舊 term 的 entry 即使已在多數節點上，也不能直接 commit，否則可能被蓋掉。所以 leader 只直接 commit 本任期的 entry | `Core::advance_commit`；[commit 規則](COMMIT_RULE_REVIEW.md) |

## 寫到磁碟與回復

| 名詞 | 白話 | 在這個專案 |
|---|---|---|
| WAL（Write-Ahead Log，預寫日誌） | 先把要改的東西寫進一個只往後加的檔案並同步到磁碟，再對外說「好了」。當機後照這個檔案恢復 | `crates/raft-storage`；[格式說明](STORAGE_FORMAT_V2.md) |
| 落盤（durable） | 資料已經同步到磁碟，當機後還在。只寫進記憶體或作業系統的快取都不算 | `durable_index` |
| fsync、`sync_all` | 要求作業系統把檔案內容真正寫到磁碟。Rust 的 `File::sync_all` 對應 Linux 的 `fsync` | 每次落盤都呼叫 |
| record、transaction | WAL 裡的一筆紀錄。每筆有 24 bytes 的 header、內容與檢查碼；內容是一次變動（transaction） | 單筆上限 2 MiB |
| CRC、CRC32C | 檢查碼：由資料算出的 32 位元數字，資料錯一個位元，數字就幾乎一定不同。CRC32C 是其中一種演算法（Castagnoli 多項式） | `crc32c`（`raft-protocol`） |
| 殘尾（torn tail） | 寫到一半就當機，檔案最後留下不完整的一筆。只有能確定是「沒寫完」的殘尾才會被截掉，截掉之前先存一份副本 | [格式說明](STORAGE_FORMAT_V2.md) §5.2 |
| fail closed | 遇到無法確定安全的情況就停下來、拒絕啟動，而不是猜一個答案繼續跑 | 回復時的各種檢查 |
| poison | 一次寫入或同步失敗後，這個 WAL 物件不再接受任何寫入，節點停止服務 | `Wal::poisoned` |
| Delta | 一步裡改變的部分：新的 term 與投票、從哪裡截斷、要附加的 entry。S5 起只寫 Delta，不寫整份狀態 | `Delta`、`Wal::save_delta` |
| barrier | core 要求「這些變動落盤之後，才能做後面的事」。在它完成前，core 不處理下一個事件 | `Effect::Persist` |
| 快照（snapshot） | 狀態機在某個 index 的完整內容。有了快照，之前的 log 就可以丟掉 | `Snapshot`；上限 64 MiB |
| 壓縮（compaction） | 建立快照後刪掉它涵蓋的 log。這裡在 WAL 超過 1 MiB、而且是上一份快照的 4 倍以上時觸發 | `Actor::maybe_compact` |
| InstallSnapshot | leader 要送的 entry 已經被壓縮掉時，改送整份快照。分塊傳送，每塊最多 256 KiB | [快照規格](SNAPSHOT_COMPACTION.md) |
| golden 檔 | 用舊版程式寫出、存進儲存庫的檔案。測試確認新版寫出的位元組和它完全相同 | `crates/raft-storage/testdata/` |

## 讀取與用戶端

| 名詞 | 白話 | 在這個專案 |
|---|---|---|
| 線性一致（linearizable） | 每個操作看起來像在「送出」與「收到回覆」之間的某一瞬間完成。讀取一定看得到在它開始前已完成的寫入 | `tools/linearizability.py` 檢查歷史紀錄 |
| ReadIndex | 不寫 log 的讀取：leader 記下當下的 commit index（readIndex），再送一輪訊息確認自己仍被多數承認，等狀態機套用到 readIndex 才回答 | [ReadIndex 規格](READ_INDEX.md)；教學 20 |
| lease read（租約讀取） | 靠時間假設跳過確認的讀取。這個專案不做，因為它依賴時鐘 | — |
| client ID、sequence | 每個 client 有一個編號，每個請求有一個遞增的序號。重送時用同一組，系統就知道是同一個請求 | [client 契約](CLIENT_CONTRACT.md) |
| 去重（deduplication） | 同一個寫入請求被送了兩次（例如回覆遺失後重試），只執行一次，第二次回傳第一次的結果 | `kv-state` 的 session |
| session | 每個 client 最近一筆寫入的紀錄：序號、命令、結果。用來去重 | `Session`（`kv-state`） |
| `TIMEOUT_UNKNOWN` | 「不知道成功了沒有」。可能已經 commit，也可能沒有。用同一組 client ID 與 sequence 重試是安全的 | client 等待上限 3 秒 |
| `NOT_LEADER` | 這台不是 leader；回覆裡的 `leader_hint` 可能指出誰是 | — |

## 成員變更

| 名詞 | 白話 | 在這個專案 |
|---|---|---|
| 成員變更（membership change） | 叢集運作中加入或移除節點 | [成員變更規格](MEMBERSHIP.md) |
| voter（投票成員） | 成員設定裡列出的節點，算在多數裡 | `Voters` |
| 成員設定（configuration） | 目前有哪些 voter。存成 log 裡的一筆 entry，寫入就生效 | `Command::Config` |
| 單步變更（single-server change） | 一次只加或移除一台，新舊設定的多數必定重疊，所以不需要 joint consensus | — |
| joint consensus | 論文的另一種做法：一次改多台，中間有一段新舊設定同時生效。這個專案不做 | — |
| bootstrap（引導） | 第一次啟動叢集時，讓一台以「只有自己」的設定開始，其他節點再逐一加入 | `--bootstrap` |
| 追趕（catch-up） | 新節點加入前，先讓它追上 leader 的 log：固定 10 輪，第 10 輪少於 4 次心跳才算追上；整個過程超過 100 次心跳就放棄 | `CATCH_UP_*` 常數 |
| 2015 年的修正 | 單步變更原本有一個反例，Ongaro 在 2015 年的 raft-dev 郵件補上規則：leader 要先在本任期 commit 一筆 entry，才能開始成員變更 | 教學 08；[論文筆記](reference/raft-notes.md) |
| PreVote、learner、leader transfer | 其他常見的擴充：選舉前先試探、不投票的追趕節點、主動交出 leader。這個專案都不做 | — |

## 效能

| 名詞 | 白話 | 在這個專案 |
|---|---|---|
| group commit | 同一輪到達的多筆寫入共用一次落盤（一次 fsync） | `Actor::run`、`Event::ProposeBatch` |
| 批次（batch） | 一次 AppendEntries 或一次提出的多筆 entry。每批最多 256 筆、512 KiB | `MAX_BATCH_ENTRIES`、`MAX_BATCH_BYTES` |
| frame（訊框） | TCP 上的一則訊息：4 bytes 的長度（big-endian）加上 JSON 內容。最多 1 MiB | `crates/raft-transport` |
| actor | 一條執行緒獨自擁有 core、WAL 與狀態機，其他執行緒只把事件交給它。這樣共識的狀態不需要鎖 | `Actor`（`raft-node`） |
| Event／Effect | core 的輸入與輸出。core 收一個 Event，回傳一串有順序的 Effect，由 actor 執行 | `raft-core` |

## 測試與驗收

| 名詞 | 白話 | 在這個專案 |
|---|---|---|
| 模擬器（simulator） | 用虛擬時間、模擬的網路與磁碟跑同一份 `raft-core`，隨機注入故障，每一步檢查不變量 | `crates/raft-sim` |
| 種子（seed） | 決定所有隨機事件的數字。同一個種子會重跑出完全相同的過程，失敗就能重現 | 驗收跑 1,000 個種子 × 10,000 個事件 |
| 不變量（invariant） | 任何時刻都必須成立的條件，例如「同一個 term 最多一個 leader」 | 模擬器的 I1～I8 |
| failpoint | 測試用的開關：讓 process 走到某個位置時自己結束，用來測「剛好在這裡當機」 | `RAFT_FAILPOINT` |
| SIGKILL | 立刻終止 process，不給它清理的機會。用來模擬當機；它不等於斷電 | — |
| proxy（代理） | 夾在兩個節點之間轉送訊息的小程式，可以丟棄、延遲、重複、調換訊息 | `tools/fault_proxy.py` |
| mutation 測試 | 故意在程式裡植入一個錯誤（mutant），看測試會不會失敗。沒失敗，代表那段程式沒有測試把關 | `scripts/mutation_check.py`，82 個案例 |
| 等價突變（equivalent mutant） | 植入的改動其實沒有改變行為，測試當然抓不到 | 教學 10 |
| 驗收契約（acceptance contract） | 一份 JSON，列出每個階段必須通過的測試與報告規則 | `acceptance/final.json` |
| receipt（收據） | 某個階段驗收通過後寫下的紀錄：命令、結果、來源指紋 | `artifacts/acceptance-final/` |
| 來源指紋（source fingerprint） | 所有原始檔案內容的雜湊。改任何一個檔案，指紋就變，舊的 receipt 就不能替新的程式作證 | `scripts/verify_gate.py` |
| 浸泡測試（soak test） | 長時間持續運作的測試，用來找只有跑久了才會出現的問題。這個專案沒有做 | — |

## 工具與環境

| 名詞 | 白話 |
|---|---|
| WSL2（Windows Subsystem for Linux 2） | 在 Windows 裡執行的 Linux。這個專案在 WSL2 的 Ubuntu 上開發 |
| ext4 | Linux 常用的檔案系統。資料要放在 WSL 自己的檔案系統（例如家目錄），不要放在 `/mnt/c` |
| tmpfs | 放在記憶體裡的檔案系統。這台 WSL 的 `/tmp` 是 tmpfs，重開就清空，也會吃記憶體 |
| cargo、rustup | Rust 的建置工具與版本管理工具 |
| venv | Python 的虛擬環境：把這個專案用的套件裝在 `.venv/`，不影響系統 |
| TOML | 設定檔格式，例如 `config/cluster.toml` |
| JSON、JSONL | 資料格式；JSONL 是每行一個 JSON，節點的紀錄用這種格式 |
| Mermaid | 用文字描述圖表的語法，GitHub 會直接顯示 |
| KV（key-value） | 以鍵查值的資料儲存方式，像一本字典 |
| P00～P18 | 驗收契約的階段編號。P00～P08 是最初的九個階段，之後每個開發階段各加一個：S1～S6 是 P10～P15，S10 是 P16，S12 是 P17，S16 是 P18（P09 沒有使用） |
| S0～S16 | 開發階段的編號，見 [開發歷程](DEVELOPMENT.md)（S7、S8、S11、S13～S15 不屬於這個 KV store） |
| CI（continuous integration，持續整合） | 每次把程式推上 GitHub，就自動在乾淨的機器上建置並跑測試，結果顯示在 README 最上面的徽章 | `.github/workflows/ci.yml` |
