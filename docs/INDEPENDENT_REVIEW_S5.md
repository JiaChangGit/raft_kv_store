# S5 獨立審查報告

日期：2026-11-15　｜　審查對象：`e5f09c2`（`main` 的 HEAD，審查時工作樹乾淨、**沒有 `s5-*` tag**），範圍是 `s1-s4-review-verified..e5f09c2` 的四個 commit　｜
審查者：沒有參與 S5 的實作　｜　依據：專案計畫的規則「獨立審查：審查者沒有參與那部分的實作，不提供預期答案」。

§1～§6、§8～§9 描述的是審查當時的 `e5f09c2`：那段期間沒有修改任何受版本控制的程式、測試或規格，所有建置、mutant 與重現都在暫存副本上進行
（副本的來源指紋與原始工作樹相同，都是 `acf55330…de9ea3`）。使用者要求把沒通過的部分在安靜的機器上重測時，發現 **F6** 與 **F7**（§7）；使用者同意修正它們，
修正與重測結果在 §11。文中的 `repro/`、`evidence/`、`spin-probe/`、`rerun/` 指 `artifacts/audit/independent-review-20261115-s5/` 底下的資料夾；
`artifacts/` 不在版本控制內，那些檔案只存在做審查的機器上，腳本裡的路徑是當時 session 的暫存目錄，要重跑時請自行改路徑。
`docs/` 計入來源指紋，所以本文件在驗收**之前**定稿；驗收的最終結果以 `scripts/status.py` 與 commit 訊息為準，不寫在這裡。

## 1. 結論

**沒有找到違反 Raft 安全性或 S5 保證（G1 落盤順序、G4 成本不隨 log 成長、G6 格式不變）的程式缺陷。**
但 **S5 不能算「已驗收」**：驗收從未跑完，文件卻已寫成完成；而且驗收會死在第 4 個 mutant（F6，S5 讓一個守衛測試失效）；修好之後 P14 的契約還有一條永遠過不了的要求（F7，檢查器不認清單）；另有幾處測試守不住一行改動就會出現的退化。
在 F1、F6、F7 解決（§11）之前不應打 `s5-performance-verified` tag。

| # | 類型 | 嚴重度 | 一句話 | 證據 |
|---|---|---|---|---|
| F6 | 測試強度退步（S5 引入）＋驗收工具缺陷 | 高 | S5 的 drain 迴圈讓 `saturated_outbound_pool_does_not_starve_status` 在對應 mutant 下不再失敗而是永遠卡住，並以每秒約 52 MB 的 trace 灌爆 `mutation_check.py` 的記憶體內擷取；P10 之後的驗收因此無法完成，三次執行都死在這裡 | 已執行（安靜機器上孤立重現，見 §7） |
| F7 | 驗收契約缺陷（S5 引入） | 高 | phase 14 要求 `write-benchmark.json` 的 `runs` 不得為空，但檢查器 `verify_gate.py` 對 `required_nonempty` 只認字串與物件，把清單一律判錯；P14 因此永遠過不了。P14 從沒跑完過，所以作者沒發現 | 已執行（整套重驗中 P14 的其餘檢查全過，只有這一條失敗，見 §7） |
| F1 | 驗收紀錄與文件不符 | 高 | tag 不存在、P14 沒跑、三份驗收報告都是舊的、驗收在第 3 個 mutant 中斷（原因見 F6）；計畫文件、作者本機的工作筆記與 PERFORMANCE.md 卻寫成「已完成、已打 tag」 | 已執行（命令與輸出見 §2） |
| F2 | 測試缺口（可能的活性退化） | 中 | follower 接收端的批次上限由 32 放寬為 256，**沒有任何測試守著**；改回 32，cargo 全部測試與 32 個整合測試都通過，但落後超過 32 筆的 follower 永遠追不上 | 已執行（mutant＋真實 process 探針） |
| F3 | 測試缺口（縱深防禦） | 中低 | 節點實際走的 `validate_delta` 有三條規則沒被測試獨立守住；S1 留下的測試走的是舊的 `validate_change` | 已執行（逐條移除規則） |
| F4 | 文件數字與證據不符 | 低 | `PERFORMANCE.md` §14.5 的「log 成長」列與 `write-benchmark.json` 對不上 | 已執行（逐項比對） |
| F5 | 測試強度 | 低 | 一個恆真的斷言；crash 測試不保證整批；node 端切批沒有測試 | 已執行（mutant）、程式閱讀 |

基準：在原始工作樹執行 `cargo test --workspace --release`，**120 個測試全部通過**；模擬器 1,000 個種子 × 10,000 個事件 **1,000/1,000 通過**。這些只代表程式在目前狀態下沒有壞，不代表 S5 已經驗收。

## 2. F1：S5 被標成完成，驗收其實沒有完成

**文件的說法**

- 計畫文件的 S5 列、作者本機的工作筆記：「S5（tag `s5-performance-verified`）已完成」。
- `docs/PERFORMANCE.md` 檔頭「驗收結果見 §14 與階段 14 的 receipt」；§14.5「以完整驗收的 `simulation.json` 為準」「以完整驗收的 `mutations.json` 為準」。
- 最後一個 commit 的訊息：「Acceptance phase 14: …」。

**實際狀態（都已執行或讀取）**

1. **tag 不存在。** `git tag -l 's5*'` 沒有輸出；現有的是 `s0`、`s1`、`s1-s4-review-verified`、`s2`、`s3`、`s4`、`v1-baseline-verified-20261112`。
2. **P14 沒有跑過。** `.venv/bin/python scripts/status.py`：P00～P13 都是 `PASS [source changed / not run]`（receipt 在 2026-11-14 03:20～03:39 UTC），**P14 是 `NOT_RUN`**。
3. **三份驗收報告的來源指紋都不是目前的來源。** 目前來源 `acf55330…`（以 `scripts/verify_gate.py` 的 `snapshot()` 算出）；

   | 報告 | 指紋 | 內容 |
   |---|---|---|
   | `artifacts/reports/simulation.json`（11-14 11:19） | `65563baf…` | 沒有 `multi_write_batches` 欄位；`reads_served` 19,271（S5 之前的數字） |
   | `artifacts/reports/mutations.json`（日期 `20261114T030903Z`） | `65563baf…` | `case_count` 49，不是 57 |
   | `artifacts/reports/write-benchmark.json`（11-15 11:23） | `093548ef…` | 在最後一個 commit 之前產生 |

   `acceptance/final.json` 的 phase 14 要求 `simulation.json` 與 `mutations.json` 的 `source_fingerprint_must_match` 為真，且 `multi_write_batches` 是正整數；以現在的檔案，phase 14 不可能通過。
4. **驗收在途中中斷。** `artifacts/runs/s5-acceptance.log`（備份在 `evidence/s5-acceptance.author.log`）第一行是 `2026-11-15T03:35:38Z`，
   而 HEAD commit 的時間是 `03:35:31Z`——驗收在 commit 後 7 秒才開始。log 的最後一行是 `reversed_queue: detected`，也就是 57 個 mutant 才做完第 3 個；
   檔案最後修改時間 11:42:14（+0800）。審查時（12:11）沒有 `verify_gate`、`mutation_check`、`cargo`、`pytest` 的 process 在跑。
   文件是在驗收開始前就寫成「完成」的。我當時以為中斷是 session 結束造成的；重測時發現停下的位置每次都是同一個（第 4 個 mutant），原因見 F6（§7）。

**我獨立重跑的部分**

| 項目 | 結果 |
|---|---|
| 模擬器 `cargo run --release --locked -q -p raft-sim -- --seeds 1000 --events 10000` | `pass_count` 1,000、`failures` 為空、`multi_write_batches` **13,785**、`reads_served` **19,180**；與 §14.2、§14.5 的數字一致（`evidence/sim_s5.json`） |
| 8 個 S5 新增的 mutant（`scripts/mutation_check.py` 指定案例名稱，在副本中） | **8/8 偵測到**，6 個 control 測試都通過（`evidence/mutation_s5.log`） |
| 3 個 S5 process 測試（副本中） | 3 passed（`evidence/pytest_perf.log`） |

所以 §14.5 引用的**主要數字是對的**，問題在於：完整的 57 個 mutant、P00～P14 的完整重驗、對應的 receipt 與 tag，都不存在。

**建議**：先處理 F6（驗收過不去的原因），再**跑一次**完整驗收，通過後才打 tag。F2～F4 會改程式、測試與文件，指紋會變，若要處理最好一起做，只跑一次驗收。
在驗收通過之前，四份文件的「已完成、tag」應視為尚未成立。

## 3. F2：接收端批次上限 32→256 沒有任何回歸防護

**背景**：S5 把 AppendEntries 的上限由 32 筆放寬為 `MAX_BATCH_ENTRIES`（256）。送出端有測試（`an_append_carries_at_most_one_batch`，並有 mutant `b_append_ignores_byte_bound` 被它抓到）；
**接收端**在 `crates/raft-core/src/lib.rs:1191` 的 `let valid_entries = entries.len() <= MAX_BATCH_ENTRIES` 沒有測試直接餵它超過 32 筆。

**實驗（都已執行）**

- mutant：只把 core 這一行改成 `entries.len() <= 32`（protocol 的 `valid_for` 不動）。
  - `cargo test --workspace`（含模擬器）：**通過**（mutant `b_follower_limit_still_32` 存活）。
  - `pytest tests/integration`（用這個 mutant 建置的節點）：**32 passed**，61.6 秒。
- 探針 `repro/test_zz_lag.py`：三節點，停掉一個 follower，寫入 100 筆小資料（不產生快照，`snapshot_index == 0`），再把它啟動並等它追上。
  - 原版：**1 passed in 1.16s**（`evidence/lag_baseline.log`）。
  - mutant：**TimeoutError**。落後的節點 2 一直是 `Candidate`、`last_applied` 0、term 一路升到 30；兩個存活節點的 `append_entries_sent` 各累積到 126,515 與 170,781，因為 leader 一直重送同一批、follower 一直拒絕（`evidence/lag_mutant.log`）。

**影響**：一行改動就能讓「落後超過 32 筆又沒有快照可用」的 follower 永遠追不上，同時不斷抬高 term 干擾叢集，而整套測試與驗收都不會發現。
這是 G4／批次複製最容易退化的一處，也是教學 07、12 講的「測試悄悄停止測試」。

**既有檢查為什麼沒抓到**：模擬器的 I1～I10 都是安全性不變量，沒有要求落後的節點最後追上；整合測試中落後追趕的案例走的是快照（`test_lagging_follower_catches_up_via_snapshot` 用 `snapshot_every=3`）。

**建議**：(1) core 單元測試：follower 收到 256 筆的 AppendEntries 成功、257 筆被拒；(2) 把 `repro/test_zz_lag.py` 這類真實 process 測試收進 `tests/integration/test_performance.py`（1.2 秒）；(3) 在 `scripts/mutation_check.py` 新增對應的 mutant 並列入 phase 14 的 `required_detected_cases`。

## 4. F3：`validate_delta` 有三條規則沒被測試獨立守住

**背景**：S5 起節點落盤走 `Wal::save_delta`，復原走 `apply_transaction`，兩者都只呼叫新的 `validate_delta`（`crates/raft-storage/src/lib.rs:306`）。
S1 留下的許多測試走的是 `Wal::save`，而 `save` 仍用舊的 `validate_change`，所以**保護不到節點實際走的這條路徑**。S5 新增的 `invalid_deltas_are_refused_without_writing`（`crates/raft-storage/src/tests.rs:1350`）是唯一的守衛。

**實驗**：在副本中逐條移除 `validate_delta` 的一條規則，執行 `cargo test -p raft-storage -p raft-node`（腳本 `repro/extra_mutants.py`）。

| 移除的規則 | 結果 |
|---|---|
| term 倒退（`delta.term < state.term`） | 偵測到（`invalid_deltas_are_refused_without_writing`） |
| 投票對象不是合法節點 | 偵測到（同上） |
| 同一 term 改票或清除投票 | 偵測到（同上） |
| entry 的 index 不銜接 | 偵測到（同上） |
| entry 的 term 大於目前 term | 偵測到（同上） |
| 同 index 同 term 但命令不同 | 偵測到（同上） |
| **entry 的 term 小於前一筆**（`entry.term < previous`） | **存活** |
| **entry 的命令不合法**（`entry.command.valid()`） | **存活** |
| **entry 的 term 為 0**（`entry.term == 0`） | **存活** |

**為什麼存活**

- 「term 小於前一筆」：測試案例 `entry term below the previous entry`（`good(2, [entry(2,3), entry(3,2,"z")])`）其實是被「同 index 同 term 不同命令」那條規則攔下的——log 裡已有 index 3、term 2、命令 `"x"`，案例給的是 `"z"`。這條規則被拿掉時案例照樣失敗，所以測不到它。
  能單獨測它的案例是 `good(4, vec![entry(4, 1, "y")])`：index 4 是新的、term 1 不超過目前 term，只有「不小於前一筆（term 2）」會擋下。
- 「命令不合法」：測試完全沒有這個案例。
- 「term 為 0」：有前一筆時，「不小於前一筆」會擋下它，所以這條只在**空 log 的第一筆 entry** 才有獨立作用；測試的 log 已有三筆。

**影響**：core 目前不會產生這三種 delta，所以這是縱深防禦的退化，不是現行 bug。
但 S1 的 `save` 路徑原本有這些保證（`DurableState::valid()` 檢查 `e.term > 0`、term 不遞減、`command.valid()`），S5 把生產路徑換成新函式之後，這些保證只剩 `save` 的舊路徑在守。

**建議**：在 `invalid_deltas_are_refused_without_writing` 補三個案例：term 倒退的新 entry（上面的 `entry(4, 1, "y")`）、`key` 超過 `MAX_KEY` 的 entry、在空 log 上 term 為 0 的第一筆 entry；並在 `mutation_check.py` 各加一個 mutant。

## 5. F4：`PERFORMANCE.md` §14.5 的「log 成長」列與 `write-benchmark.json` 不符

§14.5 表格的「log 254→1,900 筆（1 KiB 的值）」列寫「修正前 p50 5.0 → 12.4 ms；修正後 6.2 → 6.2 ms」，並說明「以完整驗收時的 `write-benchmark.json` 為準」。
`artifacts/reports/write-benchmark.json`（11-15 11:23）實際是：

| | log 254 筆 | log 1,900 筆 |
|---|---|---|
| 修正前（`s1-s4-review-verified`） | 6.2 ms | 14.8 ms |
| 修正後 | 5.1 ms | 5.5 ms |

結論（修正前隨 log 變長而變慢、修正後持平）不受影響。同一表的其他列與檔案一致（8 個用戶端 364→628 寫入／秒、落盤 480→204；32 個用戶端 400→1,362、1,920→319；重啟 476→119 ms）；
只有 1 個用戶端的修正前 p50 文件寫 5.8 ms、檔案是 5.7 ms。看起來是把另一次執行的數字混進來了。
這個檔案的指紋 `093548ef…` 也不是目前的來源（見 F1）。**建議**：改成檔案裡的數字，或重新產生報告後再抄。

## 6. F5：測試強度

- **恆真的斷言。** `tests/integration/test_performance.py::test_concurrent_writes_and_reads_stay_linearizable` 的 `status["batches"] <= status["batched_writes"]`：每一批至少一筆，所以恆為真，證明不了真實 process 中有合批。真正測合批的是節點單元測試 `queued_writes_share_one_barrier_and_status_survives_a_large_map`（96 筆排隊 → 落盤不超過 4 次）。
- **crash 測試不保證是「整批」。** `test_a_batch_persisted_but_not_answered_is_not_executed_twice` 同時送 8 筆，中斷點在第一次含 client entry 的落盤之後；那一批有幾筆取決於到達時機，測試沒有斷言它大於 1。它證明的是「至多執行一次」，不是「整批落盤後 crash 仍只執行一次」。
- **node 端切批沒有測試。** `Actor::propose`（`crates/raft-node/src/main.rs:946`）依 512 KiB 切批；把切批條件改成 `if false` 的 mutant `b_node_never_splits_proposals` 存活。
  影響不大：不切批時一批最多 256 筆，約在 1 MiB 上下，仍低於規格寫的單筆 WAL record 上限 2 MiB，而送給 follower 的批次另由 core 的 `batch_len` 依位元組切。但規格 §6.1 寫了要切，卻沒有測試證明。

## 7. F6 與 F7：重測時發現的兩個阻擋驗收的問題

### F6：S5 讓 `saturated_pool_spin` 的守衛測試失效，而那個 mutant 會拖垮機器

重測時才發現，所以排在這裡。**類型**：S5 引入的測試強度退步，加上 V1 起就有的驗收工具缺陷。**嚴重度**：高——對現有程式碼，P10 之後的每一階段都無法通過。

**現象**：完整 mutation 執行（57 個案例）每次都停在第 4 個案例 `saturated_pool_spin`：作者那次（`artifacts/mutations/20261115T033538Z` 與 `s5-acceptance.log`）、
我的第一次與第三次（`20261115T111554Z`、`20261115T114426Z`）。三個資料夾都只有 `control`、前三個 mutant 與 `saturated_pool_spin`。
（我的第二次是 WSL 重啟打斷的：之後 `uptime` 顯示 0 分鐘；第三次起改成由系統追蹤的背景工作，仍死在同一點。）

**孤立重現（已執行，`repro/probe_spin.py`，結果在 `spin-probe/results.json`）**：只跑那個測試，加上保護（虛擬記憶體 6 GB、CPU 90 秒、執行緒與可用記憶體的 watchdog），
機器安靜（起始 load 0.1～0.4，可用記憶體約 7.4 GB）。

| 程式碼 | 無 mutant | 有 mutant（`Instant::now() + PEER_TIMEOUT` 改成 `Instant::now()`） |
|---|---|---|
| S4：`s1-s4-review-verified` | 通過，1 秒 | **失敗，2.1 秒**：`pool saturation must not starve queued status: Timeout` |
| S5：`e5f09c2` | 通過，1 秒 | **100 秒內沒有結束**（用到 CPU 上限被殺）；3 個執行緒、RSS 4 MB、可用記憶體沒有變；stdout 22,103,032 行、5,238,298,876 bytes（約 52 MB/s） |

**機制（有直接證據）**：actor 迴圈的開頭先處理已到期的計時器（`rpc_deadlines` 等），處理完就 `continue`，回到迴圈開頭，**之後才**讀輸入佇列。
mutant 讓每個新送出的 RPC 的期限就是「現在」，於是重送 → 立刻到期 → 重送，永遠不會走到讀佇列那一步。

- S4：Put 與 Status 依序處理；Put 的複製一開始就進入這個空轉，Status 排在後面讀不到，測試的 `recv_timeout(1 s)` 逾時而失敗，這正是它要偵測的事。
- S5：同一輪 drain 一次取出 Put 與 Status（`docs/PERFORMANCE.md` §6.1），**先**回覆 Status、**最後**才提出寫入，所以 1 秒的檢查通過；
  接著測試送出 `Stop` 並 `worker.join()`，而 actor 已經空轉、永遠讀不到 `Stop`，測試永遠不會結束。
  我在探針副本的測試裡加兩行輸出（`--nocapture`）確認：先印出 `status answered, role="Leader"`，再印出 `Stop sent, joining the actor`，之後什麼都沒有（`spin-probe/new_mutant.probe-stderr.txt`）。

也就是說，S5 之後這個測試**抓不到它要抓的錯**，而且失敗時不是「失敗」而是「卡住」。`PERFORMANCE.md` §14.4 檢查了「依賴舊行為的測試」，但只改了因 WAL 起始狀態而失敗的部分，沒有發現強度退步——
因為會揭露它的完整 mutation 執行，剛好停在這個 mutant。

**為什麼會拖垮機器（推論，速率與時間吻合，未直接觀察到 OOM）**：`scripts/mutation_check.py` 用 `subprocess.run(..., capture_output=True, timeout=600)`，子行程的 stdout 全部留在 Python 的記憶體裡；
節點的 trace 直接寫到行程的 stdout，不受 libtest 的輸出擷取控制。52 MB/s 約 3 分鐘就是 8.6 GB（這台 WSL 的記憶體上限約 8.9 GB），而三次執行都是在第 4 個案例開始後 1～2 分鐘內中斷。
逾時殺掉的只有直接子行程（cargo），底下的測試執行檔會繼續跑。

**修正**：見 §11。要補一句限制：新的上限讓「卡住或灌輸出的 mutant」變成可讀的 `timeout`／`output_cap`（不算偵測到，驗收會失敗），而不是拖垮機器；它不會讓這種 mutant 被偵測到，後者靠修好的測試。
這也是一篇值得寫的教學（測試在失敗時卡住而不是失敗；擷取輸出沒有上限）。

### F7：phase 14 的契約有一條永遠過不了的要求（F6 修正後的整套重驗發現）

**類型**：S5 引入的驗收契約缺陷。**嚴重度**：高——不修就沒有 `PASS` 的 P14，也就沒有 tag。

**現象（已執行）**：F6 修正後在安靜的機器上整套重驗（來源指紋 `221564a1…`）：57 個 mutant 全部偵測到、模擬器 1,000/1,000、P00～P13 全部 `PASS [current source]`；
P14 的 110 個 Rust 測試與 38 個 pytest 節點也全過，只在最後的報告檢查失敗：`Report …/write-benchmark.json: runs must be nonempty`
（`artifacts/acceptance-final/phase-14.last-attempt.json`，輸出備份在 `final/phase_14.log`）。報告本身沒有問題：`runs` 是兩筆（`before`、`after`）的清單。

**原因**：契約 `reports.14` 對 `write-benchmark.json` 寫 `required_nonempty: ["command", "environment", "runs"]`；`scripts/verify_gate.py` 的 `validate_report` 對這個清單裡的鍵只認兩種型別：
`metrics`、`environment` 當物件，**其餘一律當字串**。`runs` 是清單，`isinstance(清單, str)` 為假，於是「不得為空」永遠不成立。這條要求從寫下的那一刻起就無法被滿足；
P14 從沒跑完過（審查前連 `phase-14.last-attempt.json` 都不存在），所以沒人發現。我用 `git show e5f09c2:scripts/verify_gate.py` 的檢查器驗證過：一份合法的報告（`runs` 有一筆）也被拒絕。
所以這不是 mutation 或測試強度的問題，是契約與檢查器彼此不符。

為什麼更早沒擋下：`scripts/preflight_phase.py` 只跑階段自己的測試，不檢查報告的結構。若能在不寫 receipt 的情況下，用現有報告預檢報告的結構，這類錯誤會在幾分鐘內出現，而不是整套驗收的最後一步（建議，未實作）。

**修正（使用者 2026-11-15 同意）**：見 §11。重點是**保留**「`runs` 不得為空」的要求，讓檢查器認得清單，而不是把這一項從契約拿掉。檢查器與契約都變了，依 `ACCEPTANCE_FINAL.md` 必須從 P00 全部重跑並重新產生報告。

## 8. 其他確認與疑點

**G6（格式不變）成立——我自己比對了。** `deltas_write_the_same_bytes_as_whole_states` 比的是 `save` 與 `save_delta`，兩者共用同一個 `write()`，所以釘不住「和 S5 之前相同」。
我用 `git archive s1-s4-review-verified` 取出舊版，對同一組操作（term 與投票的變化、設定與 no-op、Put 與 Delete、4,000 bytes 的值、截斷後改寫、35 筆批次）
分別用舊版 `save`、新版 `save`、新版 `save_delta` 寫出 WAL（`repro/g6_old.rs`、`repro/g6_new.rs`）：三份 `raft.wal` 都是 10,336 bytes，**sha256 相同**
（`55de43003e0d9ac8dfef87ce7b4cb2e5d6172fa5bb06120ba06da4e73040d460`，`evidence/g6-sha256.txt`）；`node.meta` 也相同。建議把這個固定輸入與位元組存成 golden 測試。

**G1（先落盤、後送出）有守住，只是沒登記成 mutation 案例。** `PERFORMANCE.md` §10 要求的 mutant「barrier 之前先送出訊息」不在新增的 8 個案例中。
我把 `Core::seal` 的 `sealed.push(barrier); sealed.extend(rest);` 對調：`membership_tests`、`snapshot_tests`、`perf_tests::a_batch_is_one_barrier_and_one_append_per_follower` 等至少 6 個 core 測試失敗。
順序是守得住的，缺的只是在契約與 `mutation_check.py` 裡列名。

**教學 14、15。** `scripts/check_lessons.py`：`lessons checked: 15; problems: 0`。兩篇的 `repro.sh` 在副本中都實際跑過（`evidence/lesson14.log`、`lesson15.log`）：
教學 15 的輸出與 `expected-output.txt` 逐項相同（`kv_bytes 1232591`）；教學 14 的毫秒數因機器而異（舊 `Cluster.invoke` 每 500 筆 p50 為 2.2 → 14.0 ms，其餘各列約 1.0～1.1 ms），形狀與預期相同。

**一個無法解釋的失敗。** 我的 mutant 流程中，有 4 次 `raft-node` 的 `tests::an_oversize_state_is_not_serialized_again_until_the_log_doubles` 失敗（對應 F3 表中的三個「term／投票」mutant 與「命令不合法」mutant 的第一次執行）。
當時機器上同時有其他建置在跑。我在機器安靜時重跑這四個 mutant：前三個被 `invalid_deltas_are_refused_without_writing` 正確偵測到，「命令不合法」則**存活**（F3 表採用的是這次的結果）；
另外 3 次沒有任何改動的 control 都通過，單獨執行這個測試 50 次（含 25 次在 24 個滿載行程下）、整個 node 測試檔 30 次（15 次滿載）都沒有再失敗。
**原因不明，沒有重現**；可能是環境的偶發問題，也可能是這個測試本身有罕見的不穩定，這裡只是記錄。
註：這個現象只會讓「存活」看起來像「偵測到」，不會反過來，所以 F3 與 F2 的存活結論不受影響。

## 9. 審查範圍與限制

**讀過的**：四個 commit 的完整 diff（`raft-core`、`raft-protocol`、`raft-storage`、`raft-node`、`raft-sim`、測試、`scripts/mutation_check.py`、`acceptance/final.json`、`tools/cluster.py`、文件與教學 14、15）；
規格 `PERFORMANCE.md` 全文；`Core::seal`、follower 的 AppendEntries 路徑、`DurableState` 的 `dirty_from` 與 `delta()`、`Wal::save_delta`、`validate_delta` 與舊 `validate_change` 的規則對照；actor 的 drain 迴圈與 `propose`。

**逐段推演後沒有找到可成立的反例**：`dirty_from` 取「本步最低被改動的 index」，截斷後重寫與只改 term／投票的情況都落在 `delta()` 的處理範圍內，快照路徑一律 `mark_clean`；
`validate_delta` 與 `validate_change` 在 S1 規則（term 不遞減、同 term 不改票、截斷範圍、index 銜接、`term ≤ 目前 term`、命令合法、同 index 同 term 不換命令）上等價（只是 F3 的三條沒測試）；
一批的寫入在 `seal` 中仍在 barrier 之後才送出訊息；actor 把寫入延到同一輪其他輸入之後提出，只改變同一輪內的處理順序；推演上不會讓已回覆的請求違反線性一致性（真實 process 的取樣檢查 `test_concurrent_writes_and_reads_stay_linearizable` 通過，但檢查器只能看 24 個操作）。

**沒有做的**：

- 審查當時：完整的 57 個 mutant（我只跑了 S5 新增的 8 個與自己加的；重測時三次完整執行都死在 F6）、P00～P13 的完整重驗、對應的 receipt。這些留給 §11 的最終驗收。
- `scripts/write_benchmark.py` 的重跑：時間數字依賴機器；我只比對了既有報告與文件，以及教學 14 的重現。
- 超過 24 個操作的真實 process 線性一致性檢查（檢查器上限）；沒有做長時間浸泡測試。
- 沒有 Raft 形式化證明，也不宣稱有；上述「沒有找到反例」只代表這次閱讀與實驗的範圍。

**可重現**：`repro/run_verify.sh`（建置副本、S5 process 測試、8 個 S5 mutant）、`repro/extra_mutants.py`（F2、F3 與 G1 的 mutant，可指定案例名稱）、
`repro/run_lag.sh` 與 `repro/test_zz_lag.py`（F2 的探針）、`repro/g6_old.rs`／`g6_new.rs`（G6 的比對）、`repro/run_lessons.sh`、`repro/probe_spin.py`（F6 的孤立重現，帶保護；
不要不加限制地重現它，一次就會寫出 5 GB 的輸出）、`repro/rerun_quiet.sh`／`rerun_quiet_2.sh`（安靜環境的補跑）。腳本中的路徑要改成你的暫存目錄。

## 10. 建議的處理順序

1. F6、F7：已處理（§11）。
2. F2、F3：補測試與 mutation 案例（會改 `crates/`、`scripts/`、`acceptance/final.json`）。
3. F4：改 §14.5 的數字，或重新產生報告。F5：把恆真的斷言改成能失敗的，並補切批的測試。
4. F1：跑**一次**完整驗收（P00～P14、重新產生 `simulation.json`、`mutations.json`、`write-benchmark.json`）；通過後才打 `s5-performance-verified`。F2～F5 若在驗收後才處理，要再驗收一次。
5. 依專案計畫，F2、F3、F6 都屬於「測試悄悄停止測試」（教學 07、12）的同類，F6 另有「工具對失控輸入沒有上限」的部分，值得各寫成教學，由使用者決定。
6. S6 的規格開始前，把本報告 §8 的「無法解釋的失敗」查清楚或確認是環境因素。

## 11. 重測與處理結果

**重測（使用者要求：把沒通過的部分在安靜的機器上再測一次，不要把背景軟體造成的失敗當成錯誤）**

| 項目 | 結果 |
|---|---|
| `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings` | 通過（`rerun/fmt.log`、`clippy.log`） |
| `scripts/preflight_phase.py 14`（7 個 Rust 測試＋3 個 pytest 節點） | 10/10 通過、0 problem（`rerun/preflight14.log`） |
| 完整 mutation 執行（F6 修正前） | 三次都死在第 4 個 mutant：作者那次、我的第一次與第三次；第二次是 WSL 重啟（`rerun/attempts.txt`）。原因查明為 F6，不是背景軟體 |
| **整套重驗（F6 修正後，來源指紋 `221564a1…`，機器安靜）** | 約 20:37～21:17，一次跑完沒有中斷（`final/progress.log`）： |
| ‑ 完整 mutation 執行 | **57/57 偵測到**，`partial` 為 false，45 個 control 全過，約 12 分鐘（`final/mutation.log`）；`saturated_pool_spin` 由修好的測試偵測到 |
| ‑ 模擬器 1,000 種子 × 10,000 事件 | 1,000/1,000 通過、`failures` 為空、`multi_write_batches` 13,785、`reads_served` 19,180 |
| ‑ 讀取與寫入的 benchmark | 重新產生，時間只記錄、不設門檻（`final/write_benchmark.log`、`read_benchmark.log`）。寫入（修正前 → 修正後）：1 個用戶端 226 → 233 次／秒；8 個用戶端 462 → 744（leader 落盤 480 → 200 次）；32 個用戶端 503 → 1,461（落盤 1,920 → 305 次）；log 由 254 筆長到 1,900 筆時 p50 修正前 4.7 → 11.9 ms、修正後 4.2 → 4.7 ms；重啟 393 → 121 ms。讀取 p50 1.56 → 1.04 ms、寫入 WAL record 數 300 → 0 |
| ‑ P00～P13 | 全部 `PASS [current source]` |
| ‑ P14 | 110 個 Rust 測試、38 個 pytest 節點、fmt、clippy、`check_lessons.py` 全過；mutation 與 simulation 兩份報告通過；**`write-benchmark.json` 的報告檢查失敗**：`runs must be nonempty`，原因是 F7（§7） |
| 背景軟體 | 當時 `dockerd`、`containerd` 各佔約一顆 CPU（`ps` 看到 load 8～18）。它們會讓時序更差，但不是 F6 的原因：第一次中斷時 load 0.1（`rerun/machine.log`），孤立重現時起始 load 0.1～0.4 |

**處理（使用者 2026-11-15 同意）**

- **F6 已修正**：`crates/raft-node/src/main.rs` 的測試 `saturated_outbound_pool_does_not_starve_status`（寫入開始複製後再問一次狀態、`batched_writes` 必須為 1、限時等待停止）；
  新增 `scripts/capped_run.py`，`scripts/mutation_check.py` 經它執行每個案例（單案例 480 秒、輸出 256 MiB 為上限，超過就殺掉整個行程群組，結果記為 `timeout` 或 `output_cap`，不算偵測到）；
  `scripts/test_mutation_check.py` 新增 5 個測試；`acceptance/final.json` 的 phase 14 加入這 5 個測試與 `saturated_outbound_pool_does_not_starve_status`，並補記 `modified_tests`。
  驗證（已執行）：未改動的程式碼下該測試通過；`saturated_pool_spin` 現在被偵測到，一個案例含 control 約 25 秒；`scripts/test_mutation_check.py` 12 個測試通過、`test_verify_gate.py` 與 `test_check_lessons.py` 21 個通過；fmt 與 clippy 乾淨。
- **F7 已修正**：`scripts/verify_gate.py` 加 `REPORT_KEY_TYPES`（`metrics`、`environment` 為物件，`runs` 為清單，其餘鍵為字串），`required_nonempty` 依它檢查，仍然要求 `runs` 不得為空；
  `scripts/test_verify_gate.py` 新增 2 個測試，`acceptance/final.json` 的 phase 14 加入這 2 個節點。驗證（已執行）：新測試通過；同一份合法報告用修正前的檢查器（`e5f09c2`）會被拒絕；
  `test_verify_gate.py`、`test_mutation_check.py`、`test_check_lessons.py` 共 35 個測試通過。使用者在這一步同意改檢查器，並同意因此從 P00 全部重跑。
- **F2～F5 未處理**（使用者決定「不修，只列進交接」）：做法與要補的案例寫在 §3～§6，S6 已全部處理（[效能規格](PERFORMANCE.md) §15）。
- **驗收與 tag**：檢查器與契約因 F7 改變，上面的整套重驗（指紋 `221564a1…`）作廢。本文件定稿後，重新產生 `mutations.json`、`simulation.json`、讀取與寫入的 benchmark，再依序重跑 P00～P14；
  全部 `PASS [current source]` 才建立本機 tag `s5-performance-verified`。**沒有這個 tag，就表示整套驗收還沒通過。** 最終結果請看 `scripts/status.py` 與 commit 訊息。
