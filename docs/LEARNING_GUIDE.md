# 從一筆 PUT 看懂 Rust Raft KV Store

想先看真實輸出，請從[實際操作帶讀](HANDS_ON.md)開始；其中的 PID、term、index、
client 結果及節點 trace 對應一次真實執行。以下 term 4／index 9 是另一個教學假設。

這份帶讀對照目前原始碼，區分三件事：**靜態追蹤**是從程式推得的路徑；
**教學假設**是為了說明而設定的 term/index；**實際觀察**必須另有命令、環境與
trace 支持。本文中的 term 4、index 9、client 7 都是教學假設，不是測量結果。
每階段真正執行的命令與限制請看 `artifacts/acceptance-v1/` 的 receipt 與輸出。

## 先建立一條路徑，而不是背完所有模組

本專案讓三個 Linux process 維持相同的 committed command 順序。每個節點只有
一個 actor 擁有 Raft state、WAL 和 KV；連線 worker 只接收、送出及轉交訊息。
simulator 與正式 runtime 呼叫同一個 Core，不另寫第二套選舉或複製演算法。

| 想回答的問題 | 先讀的實際入口 |
|---|---|
| Client 的 JSON 如何進入 actor？ | [raft-node](../crates/raft-node/src/main.rs) 的 `client_listener`、`Input::Client`、`Actor::run` |
| 請求如何變成 log？ | [raft-core](../crates/raft-core/src/lib.rs) 的 `Core::step`、`Event::Propose` |
| 哪裡等待磁碟？ | `Actor::execute` 的 `Effect::Persist`，再到 [Wal::save](../crates/raft-storage/src/lib.rs) |
| 多數回覆如何影響 commit？ | `Core::message` 的 `AppendResponse`、`advance_commit` |
| 何時改變 KV 與 session？ | [StateMachine::apply](../crates/kv-state/src/lib.rs) |
| 成功如何回到原連線？ | `Actor::reply_request`、client worker 的 reply channel |

先選表中一列，搜尋 symbol，再讀它的呼叫者與必要型別。不必每次重讀整份規格。
例如在專案根目錄搜尋：

```bash
rg -n 'Event::Propose|Effect::Persist|advance_commit' crates
rg -n 'reply_request|Input::Client|Effect::Apply' crates/raft-node
```

## 教學情境：把 x 從不存在改成 A

假設 Node 1 是 term 4 的 Leader；當前 term 的 NoOp 已完成提交，三個節點已知的
committed prefix 至少到 index 8。Node 1 的 commit_index、KV applied_index 都是 8，
key `x` 不存在。Client 7 這次送 sequence 12，並在結果確定前只重試這一個請求。

實際 Rust 型別的 JSON payload 如下；外層仍須加四個 bytes 的 big-endian 長度，
不能把這行純文字直接送進 TCP 後就當成完整 frame：

```json
{"version":1,"client_id":7,"sequence":12,"operation":{"Put":{"key":"x","value":"A"}}}
```

此範例採目前 `ClientWireRequest` 的實際 serde 表徵；version 驗證後轉為 canonical `ClientRequest`。peer Envelope 另有 version、
cluster_id、from/to/rpc_id，與 client payload 不同，不能混用。

| 步驟 | 執行者與實際程式 | 狀態與可以得出的結論 |
|---|---|---|
| 1 | client worker：`read_payload` → deserialize → `Input::Client` | 有完整且有界的輸入，尚未承諾執行 |
| 2 | actor：檢查 operation bounds、保存 waiter → `Event::Propose` | waiter 的 channel 對應原 client 連線；尚無成功 |
| 3 | core：Leader 分支 append `LogEntry` | RAM 裡新增 index 9、term 4；commit/apply 仍為 8 |
| 4 | executor：先執行 `Effect::Persist` → `Wal::save` | WAL 寫入及 sync_all 成功，durable sequence 增加；它不是 log index |
| 5 | executor：`Core::persisted(barrier_id)`，再送 AE | Leader 的 self-match 才能含 9；單獨一份尚不構成 quorum |
| 6 | Node 2 的 core/executor：核對 prev prefix、persist、回 ACK | Node 2 只承認此次驗證並持久化的 prefix；不能宣稱自己的任意 suffix |
| 7 | Node 1：驗 rpc_id/term/epoch，再更新 match | Node 1 與 Node 2 都 durable；index 9 屬目前 term，才能推進 commit |
| 8 | Node 1：`Effect::Apply` → KV `apply` | x 變為 A，session 保存 (7,12)、operation 與原結果 |
| 9 | `reply_request` → channel → client worker `write_frame` | local apply 後回 OK；Put 的原 value 為 null |

Node 3 可以較晚追上。成功不要求三份都回覆，但至少需要兩個不同合法節點的
durable 證據。未畫出的 follower commit 通知由後續 AppendEntries 帶 `leader_commit`。

正常路徑圖源在 [put_commit.mmd](../diagrams/put_commit.mmd)。圖中 `Core` 和 `執行器`
是同一個 actor thread 內的不同責任；到追隨者的箭頭才跨程序。圖省略了 timer、
Node 3、重連與失敗支線，不表示這些情境不存在。

## 五個容易混淆的時間點

1. **Append：**RAM log 出現 entry。此時 crash 後不一定存在。
2. **Local durable：**該節點的 storage save 成功；其他節點可能還沒收到。
3. **Commit：**Raft 規則允許此 entry 成為 committed prefix 的一部分；不是看 WAL 長度。
4. **Apply：**依 index 順序執行到 deterministic KV/session。
5. **Reply：**本機 apply 後才回成功；如果回覆遺失，Client 仍可能只看到 unknown。

`Core::last_applied` 記錄 core 已產生的有序 Apply intent；runtime 的
`StateMachine::applied_index()` 才是已實際完成的 state-machine index。executor
不在中途處理下一個事件，所以能維持這兩個位置的可見性邊界。

如果 `Wal::save` 失敗，executor 呼叫 `storage_failed`、回報錯誤並停止剩餘 effects；
不能把後面的 Send/Apply 繼續跑完。WAL instance 也會 poison，防止悄悄恢復服務。

## 對照規格與 Raft 論文

主要來源是 [Raft extended paper](https://raft.github.io/raft.pdf)，尤其 Figure 2、
Figure 3、§5.4.1、§5.4.2／Figure 8；專案材料則見原始
[ALL_IN_ONE_SPEC.md](../ALL_IN_ONE_SPEC.md) 的架構、持久化與 recovery 部分。

| 來源規則 | 對應程式／專案決策 | 反例或應檢查的事 |
|---|---|---|
| Figure 2 persistent state | WAL 的 term/vote/log | 只先改 RAM 就回投票，crash 後可能同 term 投另一人 |
| Figure 2 election restriction、§5.4.1 | RequestVote freshness 先比 last term，再比 index | 長但舊 term 的 log 不一定較新 |
| Figure 2 commit predicate、§5.4.2 | `advance_commit` 的 current-term＋durable quorum | 舊 term entry 在多數上不等於可直接提交 |
| Figure 3 Election Safety | unique voters、同 term 持久投票 | 同一 ACK/投票收到兩次不能算兩個節點 |
| Figure 3 Log Matching | AE 的 prev term/index、衝突 suffix 修復 | 空 heartbeat 不驗證 follower 額外 suffix |
| Figure 3 Leader Completeness | freshness＋current-term commit；跨 term checker | 只看當下 role 字串無法證明歷史 leader completeness |
| Figure 3 State Machine Safety | committed-only、連續 Apply | WAL 長度可能包含未提交 suffix |
| 專案 V1 read 決策 | 每個新 GET 都複製進 log | 舊 Leader cache 有值也不能直接回答新的 GET |
| 專案 session 決策 | 完整 request→operation/result table | 多筆 retry log entry 仍只能有一次 logical effect |

Figure 8 的五節點是論文用來展示反例的配置。正式專案始終維持三節點；閱讀或
建立獨立反例 fixture，不代表應把正式 cluster 改成五節點。關鍵推理是：
「多數裡有舊 entry」沒有提供足夠的跨 term 安全知識；當前 term 的 entry 依安全
規則提交後，前面的合法 prefix 才一起間接提交。不能因一個三節點案例暫時無事，
就刪除 current-term 條件。

角色圖源是 [raft_states.mmd](../diagrams/raft_states.mmd)。圖上的選舉 timeout
是 generation 相符的事件；舊 timer 沒有觸發轉移的權利。網路最終穩定、多數正常、
磁碟可完成，才有討論進展的前提；安全性與固定選舉時間是兩個問題。

## 磁碟回復：為什麼不直接重播所有 KV？

先讀 [STORAGE_FORMAT_V2.md](STORAGE_FORMAT_V2.md)（目前的格式；舊的格式 1 為何被換掉，見
[Bug 教學 01](lessons/01-wal-length-corruption/README.md)），再追 `Wal::recover` →
`Core::new` → 空 `StateMachine`。WAL 回復只說明該節點保存了哪些 term、vote、log；
其中仍可能有未提交 entry。Core 重啟為 Follower、commit_index=last_applied=0，
之後透過合法 Leader 的已匹配 prefix，或新 Leader 的 current-term NoOp，重新建立
提交知識。只有這時才按順序更新 KV/session。

未 sync 的完整 record 在 process crash 後可能仍在 page cache／檔案內。測試應允許
存活或遺失；真正不能丟失的是已成功返回 durable ACK 所依賴的狀態。CRC 錯誤也
不是一律「修掉最後一筆」：完整 record checksum 錯誤必須 fail closed。

可執行的相關測試命令如下。它們會建置與寫測試資料；是否已執行、結果如何，
必須以對應執行輸出為準，不能僅因本文列了命令就標示通過。

```bash
cargo test -p raft-storage vote_after_restart_not_duplicated -- --nocapture
cargo test -p raft-storage append_ack_after_durable_sync -- --nocapture
cargo test -p raft-storage uncommitted_suffix_not_applied_after_recovery -- --nocapture
cargo test -p raft-storage truncated_wal_tail_safe -- --nocapture
cargo test -p raft-storage --test crash_process -- --nocapture
```

前四個名稱是 discovery 應找到的測試；執行時仍核對實跑數量。process fixture
會在三個 WAL 邊界等待，測試再 SIGKILL 子程序；它不是第二套 Raft，也不能代替
完整三節點 runtime crash／partition 測試。

## 練習：先預測，再用來源或測試核對

**練習 A：**Leader 的 RAM log 到 9，local durable_index 只有 8，Follower 回報匹配 9。
能否把 Leader 自己計入 index 9 的 quorum？

**解答：**不能。`Core::persisted` 前不能增加 self-match。核對 `advance_commit`
的迭代上限與 executor 先 sync 後 persisted 的順序。

**練習 B：**Follower 有到 index 12，但這次 heartbeat 只核對 prev index 8，
leader_commit=12。可否 apply 自己的 9～12？

**解答：**不能。此次 confirmed prefix 最多 8，不能因本地 suffix 存在就推斷與
Leader 相符。核對 AE 的 `matched` 與 commit 上限，並看 P02 的負向測試。

**練習 C：**Client 7 的 sequence 12 PUT 已提交，Leader 在回覆前 crash；重試後
新 Leader 又產生一筆相同 request 的 log。這是否違反一次效果？

**解答：**不違反。兩筆 log 可以代表同一 logical request。session 在 apply 時
回原結果，不再次執行。可先 PUT A，再由另一 client PUT B，最後重送第一筆；
結果應保留 B，而不是把舊 A 寫回去。對照 `retry_same_request_has_single_effect`。

**練習 D：**沿用舊 GET 的 ID 得到 A，但其他 client 後來已寫入 B。這是否自動證明
stale read？

**解答：**先分辨 logical request。舊 ID 重試回原讀取結果是 duplicate semantics；
要發起新讀取必須用新 sequence，且該 GET 必須重新走安全 committed/apply 路徑。
不能把舊 ID 當作新的線性化讀取，也不能讓隔離 Leader 直接用 cache 回新成功。

**練習 E：**驗收腳本 exit 0，但 log 寫 running 0 tests。可以宣布協定正確嗎？

**解答：**連必要測試確實被執行都沒有證據。先核對 discovery、完整測試名稱、
ignore、實跑數與 assertions。測試通過仍是有限執行證據，不是所有可能執行的形式證明。

## 如何請 AI 幫忙，而不讓它只講抽象架構

可以逐次提出以下小任務，沿用已核對的來源，不必每次疊加所有 skills：

> 使用 trace-code，從一筆完整 ClientRequest 追到成功回覆。列出第一個 guard、
> 部分副作用、barrier、commit、apply、channel 與跨程序 RPC。每個結論附 symbol，
> 將靜態推論、教學假設與本次 trace 分開；不要把一個 backtrace 當作跨程序歷史。

> 只比較這次 commit predicate diff 與 §5.4.2。使用 review-change 找具體可觸發的
> 缺陷；沒有 actionable finding 就說沒有，不以風格建議湊數。

> 這次 seeded run 真的失敗了。使用 debug-root-cause，先保存 seed、首個違規、
> fault schedule 與原始 trace，再提出能區分兩個競爭假說的下一個觀察。不要把重跑
> 成功當成根因已消除。

> 以自然台灣繁體中文解釋 durability 和 commit 的差別。先用一筆 PUT 展示因果，
> 再給一個只改變 crash 時點的練習與答案。保留「尚未驗證」等限制，不用長篇口號。

## 圖源與驗證狀態

本次採用 ai-general-framework 的 sequence/state 樣板與 trace-contract，保留 Mermaid
圖源及 [PUT 時序圖](../diagrams/put_commit.svg)、[角色狀態圖](../diagrams/raft_states.svg)。
兩圖已以本機 Mermaid CLI 12.2.0、Windows Node 與 Chrome 實際渲染，並檢查 2x PNG 的
文字、箭頭與裁切；這不保證其他渲染器的版面，也不代替圖意與程式的核對。
狀態圖的「執行器停止」代表 runtime 結束，不是 Core 的 Role。Leader 收到有效同 term
AppendEntries 的退位是防禦分支；在非 Byzantine 與 Election Safety 前提下，同 term
第二個合法 Leader 不可達。原始碼對照支持圖上的路徑；仍應隨程式變更核對。

真人能否自行說明、預測相近案例與長期保留知識，需另外觀察。完成本教材、
AI 正確回答或測試通過，都不等於使用者已學會。
