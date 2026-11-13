# 第 9 節解答

**1.** 測試只斷言「狀態查詢在 1 秒內得到回覆」。S3 之後節點根本沒有東西要送，不會碰到已滿的連線池，也不會進入重試，狀態查詢自然立刻得到回覆。
不論重試邏輯是對是錯，那段程式都沒有被執行，所以結果永遠是「通過」。測試失效的方向是「看不見錯誤」，而不是「看見不存在的錯誤」。

**2.** 對照組只回答「沒有錯誤時測試會不會通過」。失效的測試在沒有錯誤時當然也通過；要看出它失效，必須問「有錯誤時它會不會失敗」，這正是 mutation 測試在問的問題。
第 3 節另外數了節點送出的 RPC，這個數字從 2 變成 0，才直接顯示出測試的情境已經不存在。

**3.** 沒有寫在任何地方。測試送出 GET，期待它引發複製，但程式碼和註解都沒有說明這個意圖；那個 `expect` 的訊息講的是結果（狀態查詢沒被餓死），不是前提（連線池確實被用到）。
沒寫出來的假設，在條件改變時不會有任何東西提醒你。修正後的註解把這個前提寫了出來。

**4.** 例如斷言「在回覆狀態查詢之前，節點至少嘗試過一次對外 RPC，並且因為連線池已滿而排入重試」。
這需要讓測試能觀察到 actor 的內部事件（例如檢查 `rpc_deadlines` 不是空的，或讀取它的 trace）。這是推論出的改法，本次沒有實作。

**5.** S3 規格 §10 列出「對既有測試的影響」時，是以「測試是否依賴 GET 的語意」為標準去找：GET 會不會進 session、會不會增加 `effect_count`、重送時回覆什麼。
這個測試不在乎 GET 的結果，只把它當成「讓 leader 去複製」的工具，因此不在那個搜尋的範圍裡。

**練習：實際檢查結果（2026-11-13）**

| 位置 | GET 的用途 | S3 之後是否仍有效 |
|---|---|---|
| `crates/raft-node/src/main.rs` 的 `saturated_outbound_pool_does_not_starve_status` | 讓 leader 去複製（工具） | **失效**，本篇的主題，已改用寫入 |
| `crates/raft-core/src/tests.rs` 的 `replicated_get_linearizable` | 直接對 core 提出一個 Get 命令 | 有效：它測的是 core 能把任何命令當成一般 entry 複製；但節點已不這樣用，驗收契約的對照表已註明 |
| `crates/raft-transport/src/lib.rs` 的 frame 測試 | 一個需要被編碼的訊息內容 | 有效：與讀取語意無關 |
| `tests/integration/test_faults.py::test_heal_recovers_and_converges` | 用 GET 回覆的 `committed_index` 等待所有節點套用 | 有效：S3 起那是 leader 讀取時已套用到的 index，仍不小於先前寫入的 index |
| `tests/integration/test_wal_corruption.py`、`test_snapshot.py` 的三個測試 | 同上，等待某個節點追上 | 有效，理由同上 |
| `tests/integration/test_durability_runtime.py::test_all_nodes_restart_relearn_commit_before_serving` | 重啟後讀取，確認資料還在 | 有效，而且更直接：ReadIndex 要先 commit 本任期 entry 才回覆，正是「重新得知 commit 才服務」 |
| `tests/integration/test_faults.py::test_partitioned_leader_cannot_commit_or_read` | 被孤立的 leader 不能回覆讀取 | 有效：S3 之後它因為無法取得多數確認而逾時 |
| `tests/integration/test_randomized.py`、`test_client_retry.py` | 讀取結果是否正確 | 有效：仍檢查讀到的值，並交給線性一致性檢查器 |
| `tools/benchmark.py` | 量測 Put/Get 的延遲 | 仍可執行，但**數字的意義變了**：GET 不再寫磁碟。比較 S3 前後的 benchmark 報告時要注意 |

結論：除了本篇的測試，沒有找到其他失效的測試；benchmark 的數字需要重新解讀。這是逐一閱讀後的判斷，不是自動檢查的結果。
