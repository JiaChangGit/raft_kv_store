# 第 9 節解答

**1.** 第一塊到達時，這則訊息的 term 5 比節點的 term 3 大，core 把 term 改成 5 並清掉投票。這一步還沒有收齊快照，所以沒有 `PersistSnapshot`；
但要落盤的狀態變了，於是產生一個普通的 `Persist` barrier，把 term 5 寫進 `raft.wal`，寫完才回覆第一塊。
最後一塊到達時，WAL 裡已經是 term 5，「快照的 term 比 WAL 新」不會發生。只有「第一塊就是最後一塊」的時候，兩件事才擠在同一個 barrier 裡。

**2.** 拿不到。leader 的 term 只出現在那則訊息裡，被終止時還沒寫進任何檔案；磁碟上唯一留下的線索是快照檔記著的「最後一筆的 term」。
它一定不大於 leader 的 term（訊息驗證時就檢查了 `last_included_term <= term`），而且一定是一個真的存在過的 term。
用它已經足以讓狀態合理；差的那一段，leader 的下一則訊息會補上。

**3.** 那一票是 term 3 投的。把 term 改成 5 卻留著它，磁碟上就變成「我在 term 5 投給了節點 3」，這件事沒有發生過。
後果（推論，沒有實測）：它不會造成重複投票，因為投票紀錄只會讓節點拒絕別人；但 term 5 若有別的候選人來要票，這台會因為一張不存在的票而拒絕，
可能讓選舉多拖一輪。正常的 term 變更都會清掉投票，回復時照做，結果才和「正常收到一則 term 5 的訊息」完全一樣，不需要另外論證。

**4.** 先改寫 WAL 的話，被終止時磁碟上是「WAL 從 index 6 之後開始，快照卻還是舊的或根本沒有」。index 6 以前的 entry 已經從 WAL 丟掉，又不在任何快照裡，
這台節點就真的少了一段已 commit 的資料。現在的順序最壞只是「快照已經有了、WAL 還留著舊的那段」，資料是多不是少。
mutation 案例 `s2_wal_rewritten_before_snapshot` 植入的就是這個順序，由真實 process 的測試抓到。

**5.** 問這個期待值是從哪裡來的。這一列測試的註解只說「不能採用」，沒有說什麼情況會產生這種磁碟狀態；
而規格 G3 說每一個中斷點都要能回復，§7.3 又把「快照比 WAL 新」列為要往前完成的情況。一個測試對「規格說要回復的那一類狀態」斷言拒絕，兩者必有一個錯。
實際做法是把規格的保證逐條對到測試，看有沒有哪個測試的期待和保證相反，再去找能不能真的產生那個狀態。這次是先推演出狀態，再用真實 process 產生它。

**練習：實際執行結果（2026-11-14）**

在暫存副本把 `SIM_CHUNKS` 改成 `[7, 7, 7]` 後執行 `cargo test -p raft-sim --test invariants simulation_randomized_invariants`：

```text
thread 'simulation_randomized_invariants' panicked at crates/raft-sim/tests/invariants.rs:29:9:
no snapshot was ever delivered in a single message
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 1 filtered out; finished in 48.06s
```

1,000 個種子的不變量 I1～I10 全部成立（否則會先在前一個斷言失敗），測試卻失敗了，失敗的是「這次執行有沒有走到單一訊息的快照」這個前提。
這正是它的用途：修正前的模擬器就是這個樣子，全部通過、卻從來沒有走到 bug 所在的路徑。把前提寫成斷言之後，「測試的設定讓某條路徑消失」會以失敗出現，而不是以綠燈出現。

如果你預測的是「會通過」，那和這個 bug 藏了兩個階段的原因是同一個：不變量成立，不代表該檢查的情況出現過。
