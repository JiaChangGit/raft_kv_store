# 帶讀：跟著一筆 PUT 走完整個系統

[架構說明](ARCHITECTURE.md) 用四張圖講系統怎麼運作；這份帶你打開程式，跟著一筆寫入從 client 走到回覆，
每一步都指出在哪個函式、那一刻什麼已經成立、什麼還沒有。最後有練習。

讀之前最好先跑過一次 [實際操作](HANDS_ON.md)，這裡用的就是那次的數字：
節點 1 是 term 2 的 leader，叢集有三台，log 已經到 index 4；client 701 用 sequence 0 寫入 `lesson-x=A`，這筆寫入成為 index 5。
內容對照 2026-11-17 的程式。

## 先找到入口，不必讀完所有模組

| 想知道的事 | 從這裡開始讀 |
|---|---|
| client 的請求怎麼進到節點？ | `crates/raft-node/src/main.rs` 的 `client_listener`、`Input::Client` |
| 同一輪的多筆寫入怎麼合併？ | `Actor::run`（一次取走已到達的輸入）、`Actor::propose`（切成批次） |
| 寫入怎麼變成 log entry？ | `crates/raft-core/src/lib.rs` 的 `Core::propose` |
| 哪裡等磁碟？ | `Core::seal` 產生 `Effect::Persist`；`Actor::execute` 呼叫 `Wal::save_delta`（`crates/raft-storage`） |
| follower 怎麼處理？ | `Core::message` 的 `Rpc::AppendEntries` 分支 |
| 什麼時候算 commit？ | `Core::advance_commit` |
| 什麼時候改到 KV？ | `Core::apply_committed` 產生 `Effect::Apply`；`StateMachine::apply`（`crates/kv-state`） |
| 怎麼回到原本的連線？ | `Actor::reply_request` |

挑一列，在專案根目錄搜尋名稱，再讀它的呼叫者：

```bash
rg -n 'fn propose|fn seal|fn advance_commit|fn apply_committed' crates/raft-core/src/lib.rs
rg -n 'Input::Client|fn propose|Effect::Persist|fn reply_request' crates/raft-node/src/main.rs
```

## 一筆 PUT 的十個步驟

client 送出的 JSON（外面還要加 4 bytes 的長度，才是一個完整的 frame）：

```json
{"version":1,"client_id":701,"sequence":0,"operation":{"Put":{"key":"lesson-x","value":"A"}}}
```

| 步驟 | 在哪裡發生 | 這一刻成立的事 |
|---|---|---|
| 1 | client 連線的執行緒：`read_payload` 讀出 frame，`canonical()` 檢查版本與長度上限，以 `Input::Client` 交給 actor，然後等回覆 | 請求格式正確；還沒有任何承諾 |
| 2 | actor：`Actor::run` 取走這一輪已到達的輸入；寫入先放進 `pending`（記住要回覆哪條連線），再由 `Actor::propose` 送出 `Event::ProposeBatch` | 這一輪只有這一筆寫入，所以批次只有一筆 |
| 3 | core：`Core::propose` 把它加進 log，成為 index 5、term 2；`replicate_all` 準備給節點 2、3 的 AppendEntries | 記憶體裡的 log 到 5；commit 仍是 4 |
| 4 | core：`Core::seal` 為這一步產生唯一一個 `Effect::Persist`，排在兩則訊息前面 | 「先落盤、再送訊息」的順序在這裡定下 |
| 5 | actor：`Actor::execute` 執行 `Persist`，`Wal::save_delta` 寫入並 `fsync`（這次約 1.8 毫秒），成功後呼叫 `Core::persisted` | 節點 1 的磁碟上有 index 5；`durable_index` 變成 5 |
| 6 | actor：送出兩則 AppendEntries，每則一條執行緒 | 訊息在路上；follower 還沒確認 |
| 7 | 節點 3：`Core::message` 核對 term、`prev_log_index` 4 與它的 term，接上 index 5，自己落盤後才回覆成功 | 節點 3 的磁碟上也有 index 5 |
| 8 | 節點 1：AppendResponse 分支核對 `rpc_id`、term 與 epoch，記下節點 3 已有 index 5，呼叫 `advance_commit` | 節點 1 自己加上節點 3，是三台中的兩台，已經是多數；而且 index 5 是本任期的 entry：commit 到 5 |
| 9 | core：`apply_committed` 產生 `Effect::Apply`；actor 呼叫 `StateMachine::apply` | `lesson-x` 變成 `A`；session 記下 client 701 的 sequence 0 與結果 |
| 10 | actor：`Actor::reply_request` 找到步驟 2 記下的連線，回覆 `OK`、舊值 `null`、committed index 5 | client 收到成功 |

節點 2 的回覆可以晚到，也可以根本沒到：步驟 8 只需要多數。它之後會從下一則 AppendEntries 的 `leader_commit` 得知 index 5 已經 commit。

## 五個容易混淆的時間點

1. **加進 log（append）：** 記憶體裡多了這筆 entry。這時當機，它可能不存在。
2. **本機落盤（durable）：** 這台節點的 `save_delta` 成功了。其他節點可能還沒收到。
3. **commit：** 依 Raft 的規則，多數已經落盤，而且是本任期的 entry。它從此不會被改掉。
4. **套用（apply）：** 依 index 順序在狀態機上執行。
5. **回覆（reply）：** leader 套用之後才回覆。回覆可能在路上遺失，client 這時只知道 `TIMEOUT_UNKNOWN`，要用同一組 ID 重送。

`Core::last_applied` 是 core 已經交出去的 `Apply` 的位置；`StateMachine::applied_index()` 才是狀態機真正套用完的位置。
actor 執行完一整串 effect 才處理下一個事件，所以在事件與事件之間，這兩個數字相同。

`save_delta` 失敗時，actor 呼叫 `Core::storage_failed`，以 `STORAGE_ERROR` 回覆所有等待中的 client，然後停止；
同一串 effect 裡後面的訊息與套用都不會執行。WAL 物件也會進入 poison 狀態，不再接受任何寫入（[WAL 格式 2](STORAGE_FORMAT_V2.md) 的 G6）。

## 對照論文

主要的來源是 [Raft 論文擴充版](https://raft.github.io/raft.pdf)，成員變更、快照與唯讀請求見 Ongaro 的博士論文；章節對照在 [論文筆記](reference/raft-notes.md)。

| 論文的規則 | 在程式裡 | 反過來想：少了它會怎樣 |
|---|---|---|
| Figure 2：term、投票、log 要落盤 | WAL 記錄 term、投票與 log 的變動 | 先回覆投票才落盤，當機重開後可能在同一個 term 投給另一台 |
| §5.4.1：投票只給 log 至少一樣新的 candidate | RequestVote 先比最後一筆的 term，再比 index | log 比較長但 term 比較舊的節點，不一定比較新 |
| §5.4.2、Figure 8：只直接 commit 本任期的 entry | `advance_commit` 的 `term_at(n) == Some(self.state.term)` | 舊 term 的 entry 在多數上，仍可能被之後的 leader 蓋掉 |
| Figure 3：log matching | AppendEntries 核對 `prev_log_index` 與它的 term，衝突時截掉後面 | 空的心跳不能證明 follower 多出來的尾巴和 leader 一致 |
| 博士論文 §6.4：唯讀請求 | ReadIndex：本任期 commit 後，記下 readIndex，等一輪多數確認 | 被隔離的舊 leader 會用自己的狀態回答，讀到舊值 |
| 博士論文第 4 章：一次一台的成員變更 | 成員設定存在 log 裡、寫入就生效；先追趕、再寫入；本任期 commit 後才開始 | 一次改兩台，新舊設定的多數可能不重疊 |
| 博士論文 §6.3：client 請求只執行一次 | 每個 client 最近一筆寫入的 session | 回覆遺失後重送，同一筆寫入會被執行兩次 |

論文 Figure 8 的劇本要五台才畫得出來，這個專案的叢集可以是 1～7 台。單元測試 `previous_term_entry_requires_current_term_commit` 不重演整個劇本，而是直接設定 leader 記下的各 follower 進度。
它先檢查：舊 term 的 entry 已在多數上時，commit index 仍然不動；再檢查：本任期的 entry 也到了多數，commit index 才前進。

## 當機之後：為什麼不直接重播整個 WAL？

重開時節點做三件事（`raft-node` 的 `main`）：

1. `Wal::open` 讀回 term、投票、log，以及最近一份快照（如果有）。
2. 有快照就用 `StateMachine::restore` 還原 KV 與 session；沒有就從空的開始。
3. `Core::restore` 以 follower 身分啟動：`commit_index` 與 `last_applied` 都設成快照的位置（沒有快照就是 0）。

WAL 裡快照之後的 entry 不會立刻套用。它們可能還沒 commit：例如舊 leader 寫了、還沒複製到多數就當機了，之後的 leader 可能把它蓋掉。
節點要等合法的 leader 用 AppendEntries 確認它的 log 一致、告訴它 commit 到哪裡，才依序套用。

這也是 [實際操作](HANDS_ON.md) 裡重開的節點要「追上」的原因：它磁碟上有 index 5，卻要等新 leader 告訴它 5 和 6 都已 commit。

相關的測試（`crates/raft-storage/src/tests.rs` 與 `crates/raft-storage/tests/crash_process.rs`）：

```bash
cargo test -p raft-storage vote_after_restart_not_duplicated
cargo test -p raft-storage append_ack_after_durable_sync
cargo test -p raft-storage uncommitted_suffix_not_applied_after_recovery
cargo test -p raft-storage truncated_wal_tail_safe
cargo test -p raft-storage --test crash_process
```

最後一組真的啟動子 process，在寫入、同步、回覆之間的指定位置用 SIGKILL 終止它，再檢查重開後的狀態。
它測的是 process 被終止，不是斷電（[自問自答](FAQ.md) Q25）。

## 練習：先預測，再用程式或測試核對

**練習 A。** leader 的記憶體裡 log 到 9，自己的 `durable_index` 只有 8；某個 follower 回報已經有 9。leader 可以把自己算進 index 9 的多數嗎？

> 不行。`Core::persisted` 之前，leader 不會把自己的 match 加到 9；`advance_commit` 只看到 `durable_index` 為止。核對步驟 5 的順序。

**練習 B。** follower 的 log 到 12，這次的 AppendEntries 只核對到 `prev_log_index` 8，`leader_commit` 是 12。follower 可以套用自己的 9～12 嗎？

> 不行。這次只證明了前 8 筆和 leader 一致，9～12 可能是舊 leader 留下、之後會被蓋掉的。
> follower 的 commit 最多推進到「這次確認一致的位置」：`leader_commit.min(matched)`，這裡是 8。

**練習 C。** client 701 的 sequence 0 已經 commit，leader 在回覆前當機。client 重送，新的 leader 又把同一個請求寫進 log。這算不算執行了兩次？

> 不算。log 裡有兩筆，代表同一個請求；狀態機套用第二筆時，看到 session 裡已有 client 701 的 sequence 0，直接回傳第一次的結果。
> 測試 `retry_same_request_has_single_effect` 寫的就是這個情況：先 PUT A，另一個 client PUT B，再重送第一筆，值保持 B。

**練習 D。** 用同一個 sequence 重送一個 GET，第一次讀到 A，第二次讀到 B（中間有人寫入 B）。這是不是讀到舊值的錯誤？

> 不是。GET 不去重，重送就是再讀一次；只要 B 在這次讀取開始前已經 commit，讀到 B 就是對的。
> 真正的錯誤是讀到「讀取開始之前就已經被覆蓋」的值。被隔離的舊 leader 不能用自己的狀態回答，就是為了避免這個（[ReadIndex 規格](READ_INDEX.md) §6）。

**練習 E。** 驗收腳本結束時 exit 0，紀錄裡卻寫著 `running 0 tests`。可以說測試都通過了嗎？

> 不行。連「要跑的測試真的跑了」都沒有證據。驗收檢查器會先列出所有測試的完整名稱，確認每個要求的測試都找得到、真的執行、沒有被略過
> （`scripts/verify_gate.py` 的 `discover_rust`）。教學 04 的工具就曾把「沒執行」報成「存活」。

## 圖與驗證的界線

[架構說明](ARCHITECTURE.md) 的四張圖是讀程式畫出的靜態路徑，不是某一次執行的紀錄。`diagrams/` 裡是 V1 時期的兩張圖，那裡的說明寫了哪些地方已經過時。

讀完、做完練習、跑過測試，代表你知道程式怎麼做；能不能自己解釋、遇到沒看過的情況能不能推出答案，要再用 [自問自答](FAQ.md) 和 [bug 教學](lessons/README.md) 的練習檢查。
