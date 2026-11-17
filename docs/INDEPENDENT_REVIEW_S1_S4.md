# S1～S4 獨立審查報告

日期：2026-11-14　｜　審查對象：`513cd58`（tag `s4-membership-verified`，工作樹乾淨）　｜
審查者：沒有參與 S2～S4 的實作（S1 例外，見 §7 的「獨立性」）　｜　依據：專案計畫的規則「獨立審查：審查者沒有參與那部分的實作，不提供預期答案」。

§1～§7 是審查當時的報告原文：審查期間沒有修改任何受版本控制的程式、測試或規格，所有實驗都在暫存副本上進行。
文中的 `repro/` 與 `evidence/` 指 `artifacts/audit/independent-review-20261114/` 底下的資料夾；`artifacts/` 不在版本控制內，那些檔案只存在做審查的機器上。
審查後使用者決定依建議修正，**處理結果在 §8**；§1～§7 描述的是修正前的 `513cd58`。

## 1. 結論

| # | 階段 | 類型 | 嚴重度 | 一句話 | 證據 |
|---|---|---|---|---|---|
| F1 | S2 | **產品缺陷** | 中高（單一節點永久無法啟動；不影響安全性） | follower 用**一則訊息**裝下「term 比自己已落盤的 term 新」的快照時，若在「快照已落盤、WAL 尚未改寫」之間被終止，之後每次啟動都以 `snapshot is newer than the durable term` 拒絕 | 已執行：真實 process、storage＋core 測試、模擬器 |
| F2 | S4（影響 S3 的證據） | **測試強度退步** | 中 | S4 把模擬器的請求一律送給 term 最高的 leader；從此不變量 I8 抓不到「沒有多數確認就回覆讀取」。`READ_INDEX.md` §13 記載的「I8 在第 266 個種子抓到」對目前的程式已不成立；這個錯誤現在只剩一個單元測試擋著 | 已執行 |
| F3 | S2 | 產品缺陷（邊界） | 低～中 | 狀態機超過 64 MiB 後，節點**每處理一個事件都重新序列化整個狀態機**（約 0.1 秒）再丟棄，和規格 N1 寫的「停止壓縮並記錄」不同 | 程式閱讀＋單次序列化量測；未做整個節點的實測 |
| F4 | S2～S4 | 測試盲點（供 S6 參考） | 低～中 | 模擬器有三個結構性的盲點，使某些程式路徑永遠走不到；F1 就藏在其中一個裡面 | 已執行（植入錯誤對照） |

沒有發現違反 Raft 安全性（選舉安全、log matching、leader completeness、狀態機安全、線性一致讀取）的缺陷。
S1 的儲存格式、S3 的讀取流程、S4 的成員變更規則，逐段閱讀與推演後沒有找到可成立的反例（範圍與限制見 §6、§7）。

基準：審查開始時在原始工作樹執行 `cargo test --workspace`，110 個測試全部通過；`scripts/status.py` 顯示 P00～P13 皆為 `PASS [current source]`。

## 2. F1：中斷的快照安裝可能讓節點永久無法啟動（S2）

**違反的規格**：`docs/SNAPSHOT_COMPACTION.md` G3「壓縮或安裝在任一步被終止，重啟後得到舊狀態或新狀態之一」，以及 §7.3 表格最後一列
（S > 0、B < S：「用 `adopt_snapshot` 得到新狀態，立刻改寫 WAL」）。這個狀態是本程式自己的寫入順序被中斷後產生的，屬於 §7.3 允許的範圍，不應該 fail closed。

**機制**

1. follower 收到 term 比自己新的 InstallSnapshot，而且整份快照在一則訊息內（`offset == 0` 且 `done`；資料不超過 256 KiB 就是如此）。
   core 在同一步裡提高 term 並安裝快照，只發出**一個** `PersistSnapshot` barrier（`crates/raft-core/src/lib.rs:1311-1317`，
   既有測試 `a_higher_term_install_persists_term_and_snapshot_together` 確認這是刻意的設計）。這時 WAL 裡還是舊的 term。
2. `Wal::install` 先讓快照檔落盤，再改寫 WAL（`crates/raft-storage/src/lib.rs:682-694`）。兩步之間 process 被終止：磁碟上是「新快照＋舊 WAL」。
3. 重啟時 `reconcile_snapshot` 用舊 WAL 的狀態呼叫 `adopt_snapshot`，term 維持舊值。快照的 term 比它大，`DurableState::valid()` 的
   `base.term <= term` 不成立，於是回傳錯誤（`crates/raft-storage/src/lib.rs:626-629`）。同樣的檔案下次啟動結果相同。

多塊的快照沒有這個問題：第一塊就讓 term 改變並落盤，最後一塊到達時 WAL 的 term 已經是新的。

**什麼時候會遇到**：某個投票成員在一次換 leader 的前後都連不上（停機或被隔離），新 leader 在這段期間壓縮掉它需要的 entry；
它回來後收到的第一則訊息就是快照（leader 當選時把它的 `next` 設在 log 尾端，之後沒收過它的回覆，壓縮後 `next - 1` 已不在 log 裡）。
再加上「快照不超過 256 KiB」與「剛好在兩次 rename 之間被終止」。時間窗很窄，但前兩個條件在小資料量下是常態，而這正是 G3 逐點列舉、用中斷點測試的那一類中斷。
新加入的節點不受影響（追趕一定從 AppendEntries 開始，term 會先落盤）。

**重現（都已實際執行）**

- 真實 process，未修改的 `target/debug/raft-node`，使用專案自己的 `tools/cluster.py` 與中斷點 `install_after_snapshot_before_wal`：
  `.venv/bin/python -B artifacts/audit/independent-review-20261114/repro/repro_install_newer_term.py . <空目錄>`

  ```
  lagging node 2 stopped at term 2 last_log_index 5 snapshot_index 3
  leader 3 term 4 snapshot_index 24 snapshot_term 4 snapshot_bytes 396
  failpoint reached: install_after_snapshot_before_wal -> SIGKILL
  files after the kill: ['node.lock', 'node.meta', 'raft.wal', 'snapshot']
  attempt 1: EXITED code=1 stderr=Error: Custom { kind: InvalidData, error: "snapshot is newer than the durable term" }
  attempt 2: EXITED code=1 stderr=Error: Custom { kind: InvalidData, error: "snapshot is newer than the durable term" }
  cluster still serves: OK
  ```
- production 的 `Core` 與 `Wal`，不經網路：`repro/review_install_newer_term.rs`（放進 `crates/raft-storage/tests/` 執行）。輸出：
  core 只要求一個 `PersistSnapshot`（term 5、base term 5），WAL 仍是 term 3；之後兩次 `Wal::open` 都被拒絕。
- 模擬器：只把 `crates/raft-sim/src/cluster.rs:9` 的 `SIM_CHUNK` 從 7 改成 4096（讓快照一則訊息送完），其餘不動，
  1,000 個種子在第 249 個失敗：重啟時 `invalid identity or durable state`，當時節點 2 的 durable term 是 89、快照 base term 是 94
  （`evidence/sim-chunk4096.json`）。

**為什麼既有的檢查沒抓到**

- `real_process_crash_during_snapshot_install_recovers` 只測本機壓縮：快照和 WAL 是同一個 term。
- 模擬器的 `SIM_CHUNK = 7` 讓每次傳送都是多塊，「一則訊息裝完」這條路徑從來沒有被執行（見 F4）。
- `MockDisk::recover`（`crates/raft-sim/src/lib.rs:125`）照抄了同一個邏輯，所以只要路徑走得到，模擬器其實抓得到。

**建議的修法（未實作，供決定）**

重啟時若快照的 term 大於 WAL 的 term，把 term 提高到快照的 term 並清除投票，再改寫 WAL。這是安全的：節點在那個較新的 term 沒有任何已落盤的投票，
被中斷的那一步的回覆也還沒送出（回覆排在 barrier 之後）；提高 term 而不投票在 Raft 中永遠安全。`MockDisk::recover` 要同步修改。
在暫存副本做了這個實驗（只改 `MockDisk::recover`，`SIM_CHUNK = 4096`）：1,000 個種子全部通過，安裝快照 16,642 次（`evidence/sim-chunk4096-termfix.json`）。
這只說明方向可行，production 的 `reconcile_snapshot` 還沒改、也沒測。
另一個做法是 core 把「提高 term」與「安裝快照」拆成兩個 barrier；改動較大，而且會讓既有測試的斷言反過來。

應補的測試：真實 process 的中斷點測試加上「快照 term 比 WAL 新」的情境；模擬器的分塊大小隨種子變化，包含一則訊息送完。
依路線圖，這個 bug 應寫成一篇教學。

## 3. F2：S4 之後，模擬器的 I8 抓不到「未經多數確認就回覆讀取」

**發生了什麼**：S3 時模擬器把用戶端請求送給「編號最小、自認為是 leader 的節點」
（`git show s3-read-index-verified:crates/raft-sim/src/cluster.rs` 第 679-680 行），它可能是已經被取代、自己還不知道的舊 leader。
S4 新增 `leader()`（`crates/raft-sim/src/cluster.rs:783-787`，取 term 最高者）給管理者用，用戶端工作負載也改用它（第 894-896 行）。
term 最高的 leader 必然是真正的 leader，所以「過期的 leader 回覆了舊資料」這個 ReadIndex 最主要的風險，在模擬器裡不再出現。
這和教學 07「測試悄悄停止測試」是同一種情況，但 `MEMBERSHIP.md` §18.3 的檢查沒有涵蓋模擬器的工作負載，
mutation 檢查也看不出來（每個植入只對應一個單元測試，不要求模擬器抓到）。

**證據（都已實際執行，暫存副本）**

| 實驗 | 結果 |
|---|---|
| 目前的模擬器＋植入 `s3_answer_without_confirmation`（拿掉 `read.round <= confirmed`） | 1,000 種子 × 10,000 事件**全部通過** |
| 模擬器改成送給 term **最低**的自認 leader（一行：`max_by_key` → `min_by_key`），production core | 全部通過（讀取 19,278 次），所以這個原本沒測到的情境裡沒有藏著產品 bug |
| 同上＋同一個植入 | 第 260 個種子失敗：`I8 Read Freshness node=1 read=13 readIndex=14 < committed 15 at start` |
| 把同一個植入編進 `raft-node`，跑 `test_faults.py`、`test_read_index.py`、`test_randomized.py` | 6 個測試**全部通過**（被完全隔離的 leader 收不到任何回覆，植入的錯誤沒有機會回覆） |

所以目前只有單元測試 `replies_to_requests_sent_before_the_read_do_not_confirm_it` 擋得住這個錯誤。產品程式本身是對的（第二列）。

**需要更正的文件**：`docs/READ_INDEX.md` §13「模擬器不變量 I8」一列（第 201 行）與 G1「模擬器以此檢查（I8）」，對 S4 之後的程式不再成立；
`acceptance/final.json` 的 `v1_mapping` 把 `simulation_randomized_invariants` 記為 `unchanged`，測試函式本身確實沒變，但它呼叫的工作負載在 S4 變弱了。

**建議**：工作負載改成在所有自認為 leader 的在線節點中隨機挑一個（管理者的請求也一樣）；加一個 process 級測試，讓舊 leader 只連得到少數節點
（例如 5 台、舊 leader 與 1 台 follower 在同一側）後對它讀取；`mutation_check.py` 的報告可加一欄「模擬器是否也抓到」，讓這類退步在驗收時看得見。

## 4. F3：狀態機超過 64 MiB 之後，每個事件都重新序列化（S2）

`crates/raft-node/src/main.rs:665-697`：`maybe_compact` 在每個事件之後執行；到了門檻就先 `self.kv.snapshot()`，超過 `MAX_SNAPSHOT` 才放棄。
`snapshot_too_large` 這個旗標只用來避免重複寫 trace，不會讓下一次跳過序列化。超過之後 WAL 只會繼續變大，門檻條件一直成立，
於是每處理一個事件（包括每 100 ms 的 heartbeat 與每一則訊息）都重做一次。

- 已執行：`repro/review_snapshot_cost.rs` 建立 16,400 筆 4 KiB 的資料（序列化後 67,473,925 bytes，剛超過上限），一次 `snapshot()` 約 90～100 ms
  （當時機器上同時有其他建置在跑，只能當作量級）。
- 推論、未實測：每個事件多 0.1 秒，節點來不及送 heartbeat，三台的資料量相同，會同時進入這個狀態。
  規格 N1 寫的是「該節點停止壓縮並記錄，log 從此恢復成長」，`limits` 寫的是 "compaction stops"；實際上是不停地做白工。
- 這個門檻要到 WAL 達到「上次快照的 4 倍」才會碰到；在那之前，§6 的 O1 多半已經先拖慢節點。

建議：放棄一次之後，記下當時的大小，等狀態機縮小或另訂條件再試，不要每個事件都試。

## 5. F4：模擬器的結構性盲點（供 S6 參考）

實驗：對 `scripts/mutation_check.py` 中位於 raft-core、raft-protocol 的每個植入錯誤，**只**跑模擬器（1,000 種子 × 10,000 事件），看模擬器自己能不能抓到。
腳本 `repro/sim_vs_mutants.py`，結果 `evidence/sim_vs_mutants.json`。專案從未宣稱模擬器能抓到其中每一個（`SNAPSHOT_COMPACTION.md` §14、`MEMBERSHIP.md` §12 都寫了限制），
所以「存活」本身不是缺陷；它說明的是模擬器看不到哪些地方，而 S6 的「所有功能同時開啟的模擬」會依賴它。

共 29 個植入：目前的模擬器抓到 12 個，17 個存活。最右欄只對讀取相關的 6 個重跑（`repro/sim_stale_vs_mutants.py`）：抓到 5 個。

| 植入的錯誤 | 專案指定的單元測試 | 目前的模擬器 | 請求改送最舊的 leader 後 |
|---|---|---|---|
| `quorum_one` | `majority_follows_the_configuration` | 抓到：committed conflict index=57 | 未跑 |
| `unsafe_old_term_commit` | `previous_term_entry_requires_current_term_commit` | **存活** | 未跑 |
| `s2_compact_beyond_applied` | `compaction_only_covers_applied_entries` | **存活** | 未跑 |
| `s2_reply_before_install_is_durable` | `follower_installs_only_after_the_snapshot_is_durable` | 抓到：append ACK before durability | 未跑 |
| `s2_install_already_covered_snapshot` | `a_snapshot_we_already_cover_is_acknowledged_without_change` | **存活** | 未跑 |
| `s2_no_append_before_base_rule` | `append_entries_that_start_before_the_base_count_as_matching` | **存活** | 未跑 |
| `s2_chunk_offset_unchecked` | `out_of_order_foreign_and_corrupt_chunks_are_refused` | **存活** | 未跑 |
| `s2_snapshot_crc_unchecked_on_receive` | `out_of_order_foreign_and_corrupt_chunks_are_refused` | **存活** | 未跑 |
| `s2_keep_suffix_on_term_mismatch` | `adopt_snapshot_keeps_suffix_only_when_the_base_matches` | 抓到：index/state invariant node=1 | 未跑 |
| `s3_answer_without_confirmation` | `replies_to_requests_sent_before_the_read_do_not_confirm_it` | **存活** | 抓到：I8 Read Freshness node=1 read=13 readIndex=14 < committed 15 at start |
| `s3_round_not_advanced` | `replies_to_requests_sent_before_the_read_do_not_confirm_it` | **存活** | 抓到：I8 Read Freshness node=1 read=13 readIndex=14 < committed 15 at start |
| `s3_skip_current_term_commit_wait` | `read_waits_for_a_committed_entry_of_its_term` | 抓到：I8 Read Freshness node=4 read=25 readIndex=20 < committed 22 at start | 抓到：I8 Read Freshness node=4 read=25 readIndex=20 < committed 22 at start |
| `s3_keep_reads_after_losing_leadership` | `reads_fail_when_leadership_is_lost` | **存活** | 抓到：I8 Read Freshness node=4 read=37 readIndex=28 < committed 29 at start |
| `s3_answer_before_applied` | `read_waits_until_the_state_machine_reaches_read_index` | **存活** | **存活** |
| `s4_no_2015_fix` | `the_2015_counterexample_cannot_lose_a_committed_entry` | 抓到：I9 change rule node=3 index=47 term-commit=false previous-committed=tr | 未跑 |
| `s4_two_changes_at_once` | `change_requests_are_checked_and_one_runs_at_a_time` | **存活** | 未跑 |
| `s4_change_over_uncommitted_configuration` | `change_requests_are_checked_and_one_runs_at_a_time` | **存活** | 未跑 |
| `s4_judge_uncommitted_before_term_commit` | `a_new_leader_waits_for_a_committed_entry_of_its_term` | **存活** | 未跑 |
| `s4_count_votes_from_outside` | `only_voters_of_the_configuration_count` | 抓到：leader node=1 term=8 lacks a durable vote majority of 2,3 | 未跑 |
| `s4_leader_counts_itself_outside_configuration` | `removing_a_follower_and_then_the_leader` | 抓到：I4 Leader Completeness node=3 term=3 index=26 | 未跑 |
| `s4_no_step_down_after_removal` | `removing_a_follower_and_then_the_leader` | **存活** | 未跑 |
| `s4_removed_node_campaigns` | `removing_a_follower_and_then_the_leader` | 抓到：I10 campaign without the right node=5 | 未跑 |
| `s4_campaign_without_configuration` | `a_node_without_configuration_never_campaigns` | 抓到：I10 campaign without the right node=3 | 未跑 |
| `s4_no_disruption_guard` | `a_recent_leader_contact_blocks_votes_without_a_term_change` | **存活** | 未跑 |
| `s4_add_without_catch_up` | `a_new_node_is_found_in_one_round_trip_and_added_after_ten_rounds` | 抓到：leader node=1 term=2 lacks a durable vote majority of 1,2 | 未跑 |
| `s4_read_index_fixed_majority` | `read_index_needs_a_majority_of_the_current_configuration` | **存活** | 抓到：I8 Read Freshness node=4 read=37 readIndex=28 < committed 29 at start |
| `s4_snapshot_records_latest_configuration` | `a_snapshot_records_the_configuration_at_its_position` | 抓到：I6 snapshot node=2 index=16 records the wrong configuration | 未跑 |
| `s4_no_length_hint` | `a_new_node_is_found_in_one_round_trip_and_added_after_ten_rounds` | **存活** | 未跑 |
| `s4_apply_after_barrier` | `applies_stay_in_order_when_a_barrier_commits_more` | 抓到：apply order/commit bound node=2 index=69 | 未跑 |

從程式與上表歸納出三個原因：

1. **快照永遠分成多塊**（`SIM_CHUNK = 7`）。一則訊息裝完、整份快照重複送達這些路徑不會出現。F1 在這裡；
   `s2_install_already_covered_snapshot` 存活也是因為重複的最後一塊在多塊傳送下會先被「沒有進行中的接收」擋掉。
2. **請求只送給 term 最高的 leader**（F2）。過期 leader 上的讀取、寫入、成員變更都不會出現。
3. **管理者太守規矩、用戶端太早結束**。管理者一次只送一個請求、等回覆或 15 秒逾時，所以「同時兩個變更」「蓋過未 commit 的設定」不會出現；
   用戶端每個種子只有 64 個操作（`next_client < 64`，V1 起如此），大約前四分之一的事件就用完，之後只剩成員變更與 NoOp。
   驗收報告 `artifacts/reports/simulation.json` 的數字是每個種子平均約 19 次讀取。

## 6. 觀察（不列為缺陷）

- **O1（S2，推論、未量測）**：`SNAPSHOT_COMPACTION.md` 第 50 行說壓縮讓「複製成本因此有上限」。這個上限是「max(1 MiB, 上次快照的 4 倍)」，和資料量成正比。
  core 每一步開頭 clone 整份狀態（`lib.rs:413`）、`Persist` 再 clone 一次，`Wal::save` 逐筆比對整份 log，回復時 `apply_transaction` 每筆 record clone 一次（對 record 數是平方）。
  資料量到幾十 MiB 時每個事件就要搬動上百 MiB。這是已決定延到 S5 的 D4，這裡只提醒：S5 量測時要把快照大小當變數，而不只是 log 筆數。
- **O2（S4）**：leader 在同一步裡 commit 並因為被移除而退位時，那一步 commit 的寫入，用戶端收到的是 `TIMEOUT_UNKNOWN` 而不是 `OK`
  （`main.rs:537` 以執行效果當下的角色決定是否回覆，這時角色已經是 follower）。安全（用戶端以同一個編號重試會得到原結果），但可以改成只要有人在等就回覆。
- **O3（V1 起，推論、未實測）**：一筆 AppendEntries 遺失後，leader 要等 375 ms 才重送，而最小選舉逾時是 400 ms。
  估算單一封包遺失約有 19% 的機率（75/401）讓該 follower 先逾時參選；干擾防護會讓它選不上，但它提高的 term 會在下一次回覆時讓 leader 退位一次。
  沒有 PreVote 是已聲明的取捨，這裡只是把數字寫出來。
- **O4（S2，文件）**：`SNAPSHOT_COMPACTION.md` §9.3 第 6 步寫 core「在 `persisted` 後把 commit_index、last_applied 推到 S」；
  實作是在同一步先推進、回覆排在 barrier 之後（`lib.rs:1315-1316`）。barrier 期間 core 不處理事件，對外等價，文件可以照實作更新。
- **O5（契約）**：`v1_mapping` 有 5 個測試註記 `unchanged` 或 `same test body`，內容其實有機械性的修改（`DurableState::new`、新增欄位 `last_log_index`、搬到 `tests.rs` 後的縮排）：
  `stale_candidate_rejected`、`tcp_framing_handles_partial_io`、`rpc_correlation_ignores_stale_reply`、`append_ack_after_durable_sync`、
  `uncommitted_suffix_not_applied_after_recovery`。逐一看過差異，斷言沒有變弱；註記可以寫精確一點。

## 7. 審查範圍、方法與限制

**讀過全文的**：`raft-core/src/lib.rs`、`raft-protocol/src/lib.rs`、`raft-storage/src/lib.rs`、`raft-node/src/main.rs`、`raft-sim/src/{lib,cluster}.rs`、
`kv-state/src/lib.rs` 的非測試部分、`tools/linearizability.py`、`tests/integration/` 的 `test_faults.py`、`test_read_index.py`、`test_randomized.py`、
`scripts/mutation_check.py` 的案例表、計畫文件、[工程規則](ENGINEERING_RULES.md)、`docs/ACCEPTANCE_FINAL.md`。

**只讀了部分的**：四份規格的「保證與不保證」、一致性規則與實作補記（沒有逐節核對全文）；`tools/cluster.py`；`acceptance/final.json` 的對照表與註記；core 的測試只看名稱與少數內容。

**沒有看的**：十篇教學的內容、`raft-transport`（V1）、`tools/fault_proxy.py`、`tools/kv_admin.py` 的細節、`scripts/verify_gate.py` 等驗收程式的邏輯、
`raft-storage/src/tests.rs` 與 `crash_process.rs` 的逐條斷言、benchmark。沒有重跑完整驗收，也沒有跑整套 pytest（只跑了 §3 表中那 6 個）。

**對「v1 的每一項測試都有不較弱的對應」這項責任**（`ACCEPTANCE_FINAL.md`）：以腳本比對了 27 個 v1 測試在 V1 tag 與 HEAD 的函式內容。
12 個完全相同；1 個被取代；14 個有修改，其中 9 個已記在 `modified_tests` 或對照表的註記、5 個是 O5 的機械性修改。
已記錄的那 9 個，我只讀了註記，沒有逐一核對「index 與 term 平移後斷言相同」的說法。唯一確認變弱的是模擬器（F2），而它的測試函式本身沒有改。

**獨立性**：審查者沒有參與 S2～S4 的實作，符合計畫的要求。
**S1 不完全符合**：S1 的規格草稿與第一個 WIP commit（`3d1857b`、`7123c68`）也出自這位審查者，所以對 S1 來說這不是獨立的審查；若要嚴格符合，S1 需要另一位審查者再看一次。
審查開始時只讀了專案目的與階段順序，沒有讀先前的工作紀錄、教學或任何「預期會找到什麼」的資料。

**其他限制**：這是一次有限的人工閱讀加上針對性的實驗，不是形式化驗證；「沒有發現」只代表在上述範圍內沒有找到。
我自己的對照實驗第一次執行時，重蹈了教學 06 的錯誤（各個植入共用一個建置目錄，前一個植入殘留到後面），徵兆是互不相干的植入都回報同一個 `committed conflict index=57`。那批結果已作廢、沒有保留；
本報告的數字都來自修正後、每個案例重建所有 crate 的執行。

## 8. 處理結果（2026-11-14，作者指示「修，照建議做」）

| # | 處理 | 新增或修改的測試 | 教學 |
|---|---|---|---|
| F1 | 已修正。重啟時快照的 term 大於 WAL 的 term：採用快照的 term、清除投票，再改寫 WAL（`raft-storage` 的 `reconcile_snapshot`；模擬器的 `MockDisk::recover` 同步）。規格 `SNAPSHOT_COMPACTION.md` G3、§7.3 已修訂 | 新增 `interrupted_install_from_a_newer_term_raises_the_term`、`real_process_crash_during_install_from_a_newer_term_recovers`；`snapshot_and_wal_consistency_rules` 最後一列原本斷言「拒絕」，已改；mutation 案例 `s2_recover_refuses_newer_term_snapshot` | [11](lessons/11-install-from-a-newer-term/README.md) |
| F2 | 已修正模擬器。請求在所有自認為 leader 的在線節點中隨機挑選；同時存在兩個自認的 leader 時，以 1/4 的機率對 term 較低者另發一筆讀取。只做隨機挑選是不夠的：實測那樣 1,000 個種子只有 43 次請求送到過期 leader，植入的錯誤仍然全部通過 | 模擬器測試斷言 `stale_leader_requests > 0`；mutation 案例 `sim_must_catch_unconfirmed_read`（要求模擬器自己抓到）；真實 process 測試 `test_a_leader_left_with_a_minority_does_not_answer_reads`（植入錯誤時舊 leader 回覆了 `OK` 與舊值 `old`，測試失敗；其餘 5 個讀取與故障測試仍通過） | [12](lessons/12-simulator-stopped-asking-stale-leaders/README.md) |
| F3 | 已修正。放棄一次後，要等「上次快照以來已套用的筆數加倍」才再試 | 新增 `an_oversize_state_is_not_serialized_again_until_the_log_doubles`；mutation 案例 `s2_oversize_snapshot_serialized_every_event` | [13](lessons/13-oversize-snapshot-serialized-every-event/README.md) |
| F4-1 | 已修正。快照分塊大小隨種子在 7、13、4096 bytes 間變化；follower 安裝途中 crash 的機率由 1/100 提高到 1/10（只變化分塊時，1,000 個種子裡 F1 的巧合一次都沒出現） | 模擬器測試斷言 `one_message_snapshots > 0`；mutation 案例 `sim_must_catch_interrupted_newer_term_install` | 11 |
| F4-2 | 同 F2 | | 12 |
| F4-3 | **未處理，留給 S6**：管理者一次只送一個請求；用戶端每個種子 64 個操作 | 已寫入契約的 `limits` 與 `MEMBERSHIP.md` §18.5 | |
| O1 | 未處理（S5 的範圍）；`SNAPSHOT_COMPACTION.md` 補了一句說明上限隨資料量成長 | | |
| O2 | 未處理。安全，只是回覆 `TIMEOUT_UNKNOWN` 而不是 `OK` | | |
| O3 | 未處理。沒有 PreVote 是已聲明的取捨；19% 是估算，沒有實測 | | |
| O4 | 已更新 `SNAPSHOT_COMPACTION.md` §9.3 第 6 步的文字 | | |
| O5 | 已把契約 `v1_mapping` 中 5 個不精確的註記改寫，並把 `simulation_randomized_invariants` 記入 `modified_tests` | | |

修正後另外確認的事（都已執行）：

- 真實叢集的重現腳本（§2）：修正前兩次啟動都被拒絕；修正後兩次都啟動，term 4、快照位置 24，並追上其他節點。
- F3 的量測在沒有其他工作的機器上重做：67 MiB 的狀態機序列化一次約 50 ms（§4 的 90～100 ms 是和其他建置同時跑的結果）；修正前 10 個事件花 500 ms，修正後 52 ms。
- 修正後的模擬器以 production core 跑 1,000 個種子 × 10,000 個事件全部通過。統計以 `artifacts/reports/simulation.json` 為準。
- 四個新的 mutation 案例都被抓到；完整的 mutation 報告以 `artifacts/reports/mutations.json` 為準。

仍然成立的限制：S1 的審查只做到「不同 session」（§7）；§7 列出的「沒有看的」部分這次也沒有補看。
